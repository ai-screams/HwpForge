use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::table::{grid::TableGrid, Table};
use hwpforge_foundation::BorderLineType;
fn read(styles: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
fn tables(d: &DocxDecoded) -> Vec<Table> {
    let mut values = Vec::new();
    d.document.for_each_table(|t| values.push(t.clone()));
    values
}
fn cell(pr: &str, text: &str) -> String {
    format!("<w:tc><w:tcPr>{pr}</w:tcPr><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc>")
}
const ZERO_MARGIN_STYLE: &str = "<w:style w:type='table' w:default='1' w:styleId='zero'><w:tblPr><w:tblCellMar><w:left w:w='0'/><w:right w:w='0'/></w:tblCellMar></w:tblPr></w:style>";
fn grid() -> &'static str {
    "<w:tblGrid><w:gridCol w:w='20'/><w:gridCol w:w='20'/><w:gridCol w:w='20'/></w:tblGrid>"
}
// 이것을 실패시키는 것: 생략 열을 삭제하거나 유도 좌표만 검사.
#[test]
fn omitted_columns_keep_coordinates() {
    for omitted in [true, false] {
        let first = if omitted {
            "<w:trPr><w:gridBefore w:val='1'/></w:trPr>".into()
        } else {
            cell("", "")
        };
        let d = read(
            "",
            &format!(
                "<w:tbl>{}<w:tr>{first}{}{}</w:tr></w:tbl>",
                grid(),
                cell("", "A"),
                cell("", "B")
            ),
        );
        let ts = tables(&d);
        let t = &ts[0];
        assert_eq!(TableGrid::from_table(t).unwrap().dimensions(), (1, 3));
        assert_eq!(
            t.rows[0]
                .cells
                .iter()
                .map(|c| (c.col_span, c.row_span, c.paragraphs[0].text_content()))
                .collect::<Vec<_>>(),
            [(1, 1, "".into()), (1, 1, "A".into()), (1, 1, "B".into())]
        );
        assert_eq!(
            d.warnings.iter().any(|w| w.code() == "DOCX_TABLE_POSITION_APPROXIMATED"),
            omitted
        );
    }
}
// 이것을 실패시키는 것: tblGrid 없는 병합의 열 수를 셀 개수로 추정.
#[test]
fn inferred_grid_spans() {
    let d = read(
        "",
        &format!(
            "<w:tbl><w:tr>{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",
            cell("<w:gridSpan w:val='2'/>", "A"),
            cell("", "B"),
            cell("", "C")
        ),
    );
    let ts = tables(&d);
    let g = TableGrid::from_table(&ts[0]).unwrap();
    assert_eq!(g.dimensions(), (2, 2));
    assert_eq!(
        g.iter_anchors()
            .map(|c| (c.anchor.row, c.anchor.col, c.row_span, c.col_span))
            .collect::<Vec<_>>(),
        [(0, 0, 1, 2), (1, 0, 1, 1), (1, 1, 1, 1)]
    );
}
// 이것을 실패시키는 것: orphan 삭제, 비인접 병합, continuation 본문 보존.
#[test]
fn vertical_merge_intervals() {
    let d = read(
        "",
        &format!(
            "<w:tbl><w:tr>{}</w:tr><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
            cell("<w:vMerge w:val='restart'/>", "A"),
            cell("<w:vMerge/>", "discard"),
            cell("<w:vMerge w:val='continue'/>", "")
        ),
    );
    let ts = tables(&d);
    assert_eq!(ts[0].rows[0].cells[0].row_span, 3);
    assert!(ts[0].rows[1].cells.is_empty());
    assert!(d.warnings.iter().any(|w| w.loss() == "merge_continuation" && w.action() == "omit"));
    let d = read("", &format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell("<w:vMerge/>", "A")));
    assert_eq!(tables(&d)[0].rows[0].cells[0].paragraphs[0].text_content(), "A");
    assert!(d.warnings.iter().any(|w| w.loss() == "vertical_merge"));
}
// 이것을 실패시키는 것: 행 초과/구간 불일치/0 span을 성공으로 바꿈.
#[test]
fn invalid_grid_intervals() {
    for body in [
        format!("<w:tbl>{}<w:tr>{}</w:tr></w:tbl>", grid(), cell("<w:gridSpan w:val='4'/>", "A")),
        format!(
            "<w:tbl><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
            cell("<w:gridSpan w:val='2'/><w:vMerge w:val='restart'/>", "A"),
            cell("<w:vMerge/>", "B")
        ),
        format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell("<w:gridSpan w:val='0'/>", "A")),
        "<w:tbl/>".into(),
    ] {
        assert!(decode_docx(&package(&body, &[])).is_err());
    }
}
// 이것을 실패시키는 것: 기본 표 스타일 미지정을 Solid로 처리하거나 출처별 중복 등록.
#[test]
fn equivalent_border_sources() {
    let none="<w:left w:val='none'/><w:right w:val='nil'/><w:top w:val='none'/><w:bottom w:val='none'/><w:insideH w:val='none'/><w:insideV w:val='none'/>";
    let styles=format!("<w:style w:type='table' w:default='1' w:styleId='base'/><w:style w:type='table' w:styleId='child'><w:basedOn w:val='base'/><w:tblPr><w:tblBorders>{none}</w:tblBorders></w:tblPr></w:style>");
    let body=format!("<w:tbl><w:tr>{}</w:tr></w:tbl><w:tbl><w:tblPr><w:tblStyle w:val='child'/></w:tblPr><w:tr>{}</w:tr></w:tbl><w:tbl><w:tblPr><w:tblBorders>{none}</w:tblBorders></w:tblPr><w:tr>{}</w:tr></w:tbl><w:tbl><w:tr>{}</w:tr></w:tbl>",cell("","A"),cell("","B"),cell("","C"),cell(&format!("<w:tcBorders>{none}</w:tcBorders>"),"D"));
    let d = read(&styles, &body);
    for t in tables(&d) {
        assert_eq!(t.rows[0].cells[0].border_fill_id, Some(1));
    }
    let b = &d.registry.border_fill(1).unwrap().border;
    assert_eq!(
        [b.left.line_type, b.right.line_type, b.top.line_type, b.bottom.line_type],
        [BorderLineType::None; 4]
    );
    assert!(d.registry.border_fill(2).is_none());
}
fn small_table(width: &str, inner: &str) -> String {
    format!("<w:tbl><w:tblPr>{width}</w:tblPr><w:tr>{inner}</w:tr></w:tbl>")
}
// 이것을 실패시키는 것: 잔여를 앞 열에 넣거나 중간 값을 정수로 절삭.
#[test]
fn width_rounding_and_conflicts() {
    for (width, expected) in [("20", [33, 33, 34]), ("20.2", [34, 34, 33])] {
        let d = read(
            ZERO_MARGIN_STYLE,
            &small_table(
                &format!("<w:tblW w:type='dxa' w:w='{width}'/>"),
                &cell("", "A").repeat(3),
            ),
        );
        assert_eq!(
            tables(&d)[0].rows[0].cells.iter().map(|c| c.width.as_i32()).collect::<Vec<_>>(),
            expected
        );
    }
    let d = read(
        ZERO_MARGIN_STYLE,
        &format!(
            "<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='20'/></w:tblPr>{}<w:tr>{}</w:tr></w:tbl>",
            grid(),
            cell("<w:tcW w:type='dxa' w:w='50'/>", "A").repeat(3)
        ),
    );
    assert_eq!(
        tables(&d)[0].rows[0].cells.iter().map(|c| c.width.as_i32()).collect::<Vec<_>>(),
        [33, 33, 34]
    );
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_TABLE_WIDTH_APPROXIMATED"));
    assert!(decode_docx(&package(
        &small_table("<w:tblW w:type='dxa' w:w='0.2'/>", &cell("", "A").repeat(3)),
        &[]
    ))
    .is_err());
}
// 이것을 실패시키는 것: printable_width 사용 또는 pct를 원본 숫자로 계산.
#[test]
fn relative_width_and_landscape() {
    let d=read(ZERO_MARGIN_STYLE,&format!("{}<w:sectPr><w:pgSz w:w='15840' w:h='12240' w:orient='landscape'/><w:pgMar w:left='1440' w:right='1440'/></w:sectPr>",small_table("<w:tblW w:type='pct' w:w='2500'/>",&cell("","A"))));
    assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 64800);
    let d=read(ZERO_MARGIN_STYLE,&small_table("<w:tblW w:type='dxa' w:w='200'/>",&format!("<w:tc><w:tcPr><w:tcMar><w:left w:type='dxa' w:w='20'/><w:right w:type='dxa' w:w='20'/></w:tcMar></w:tcPr>{}</w:tc>",small_table("<w:tblW w:type='pct' w:w='2500'/>",&cell("","N")))));
    let ts = tables(&d);
    assert_eq!(ts[0].width.unwrap().as_i32(), 1000);
    assert_eq!(ts[1].width.unwrap().as_i32(), 800);
    let d = read(
        ZERO_MARGIN_STYLE,
        &small_table(
            "<w:tblW w:type='dxa' w:w='200'/>",
            &format!(
                "<w:tc>{}</w:tc>",
                small_table("<w:tblW w:type='pct' w:w='2500'/>", &cell("", "N"))
            ),
        ),
    );
    assert_eq!(tables(&d)[1].width.unwrap().as_i32(), 1000);
}
// 이것을 실패시키는 것: 상위 테두리의 내부/외부 면 선택 반전 또는 cell override 무시.
#[test]
fn inherited_grid_and_conflicting_faces() {
    let edges="<w:left w:val='single'/><w:right w:val='single'/><w:top w:val='single'/><w:bottom w:val='single'/><w:insideH w:val='single'/><w:insideV w:val='single'/>";
    let styles=format!("<w:style w:type='table' w:styleId='parent'><w:tblPr><w:tblBorders>{edges}</w:tblBorders></w:tblPr></w:style><w:style w:type='table' w:styleId='selected'><w:basedOn w:val='parent'/></w:style>");
    let d = read(&styles, &small_table("<w:tblStyle w:val='selected'/>", &cell("", "A")));
    let t = &tables(&d)[0];
    let b = &d.registry.border_fill(t.rows[0].cells[0].border_fill_id.unwrap()).unwrap().border;
    assert_eq!(
        [b.left.line_type, b.right.line_type, b.top.line_type, b.bottom.line_type],
        [BorderLineType::Solid; 4]
    );
    let d = read(
        &styles,
        &small_table(
            "<w:tblStyle w:val='selected'/>",
            &format!(
                "{}{}",
                cell("<w:tcBorders><w:right w:val='none'/></w:tcBorders>", "A"),
                cell("", "B")
            ),
        ),
    );
    let ts = tables(&d);
    let cells = &ts[0].rows[0].cells;
    assert_eq!(
        d.registry.border_fill(cells[0].border_fill_id.unwrap()).unwrap().border.right.line_type,
        BorderLineType::None
    );
    assert_eq!(
        d.registry.border_fill(cells[1].border_fill_id.unwrap()).unwrap().border.left.line_type,
        BorderLineType::None
    );
    assert!(d.warnings.iter().any(|w| w.loss() == "border_conflict"));
}
// 이것을 실패시키는 것: 배경을 Core에 남기거나 행 높이 범위를 무시.
#[test]
fn omitted_decoration_and_height() {
    let d=read("",&format!("<w:tbl><w:tr><w:trPr><w:trHeight w:val='240' w:hRule='exact'/></w:trPr>{}</w:tr></w:tbl>",cell("<w:shd w:fill='FFFF00'/>","A")));
    let ts = tables(&d);
    assert_eq!(ts[0].rows[0].cells[0].background, None);
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_CONTENT_LOST"
        && w.loss() == "cell_background"
        && w.action() == "omit"));
    assert!(d.warnings.iter().any(|w| w.loss() == "row_height"));
    for value in ["-1", "20000001", "oops"] {
        let body=format!("<w:tbl><w:tr><w:trPr><w:trHeight w:val='{value}' w:hRule='atLeast'/></w:trPr>{}</w:tr></w:tbl>",cell("","A"));
        assert!(decode_docx(&package(&body, &[])).is_err());
    }
}
// 이것을 실패시키는 것: 셀 sectPr가 구역을 닫거나 셀 안 강제 나누기 소거.
#[test]
fn cell_layout_keeps_content_and_fragments() {
    let d=read("",&small_table("","<w:tc><w:p><w:pPr><w:sectPr><w:pgSz w:w='1'/></w:sectPr></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t></w:r></w:p></w:tc>"));
    assert_eq!(d.document.sections().len(), 1);
    assert_eq!(
        tables(&d)[0].rows[0].cells[0]
            .paragraphs
            .iter()
            .map(|p| (p.text_content(), p.page_break))
            .collect::<Vec<_>>(),
        [("A".into(), false), ("B".into(), true)]
    );
    for loss in ["nested_section", "cell_break"] {
        assert!(d.warnings.iter().any(|w| w.loss() == loss));
    }
}
// 이것을 실패시키는 것: 표 경계에서 번호 상태를 초기화하거나 셀 내어쓰기 별도 경로 사용.
#[test]
fn lists_continue_across_tables() {
    let numbering=format!("<w:numbering xmlns:w='{W}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num></w:numbering>");
    let p="<w:p><w:pPr><w:numPr><w:ilvl w:val='0'/><w:numId w:val='7'/></w:numPr></w:pPr><w:r><w:t>item</w:t></w:r></w:p>";
    let body = format!("{}{}", small_table("", &format!("<w:tc>{p}{p}</w:tc>")), p);
    let d = decode_docx(&package(&body, &[("numbering", "numbering.xml", &numbering)])).unwrap();
    let mut lists = Vec::new();
    d.document.for_each_paragraph(|p| {
        let shape = &d.registry.para_shapes[p.para_shape_id.get()];
        if let Some(list) = shape.list {
            lists.push((list, shape.indent_left.as_i32(), shape.indent_first_line.as_i32()));
        }
    });
    assert_eq!(lists.len(), 3);
    assert_eq!(lists[0], lists[1]);
    assert_eq!(lists[1], lists[2]);
    assert_eq!((lists[0].1, lists[0].2), (1800, 0));
}
// 이것을 실패시키는 것: gridAfter 채움 누락, 빈 셀에 목록 또는 이전 문단 서식 복사.
#[test]
fn omitted_tail_and_synthetic_paragraphs_have_no_markers() {
    let styles="<w:style w:type='paragraph' w:default='1' w:styleId='body'><w:pPr><w:outlineLvl w:val='0'/><w:spacing w:before='100' w:after='200'/></w:pPr></w:style>";
    let d = read(
        styles,
        &format!(
            "<w:tbl>{}<w:tr><w:trPr><w:gridAfter w:val='2'/></w:trPr>{}</w:tr></w:tbl>",
            grid(),
            cell("", "A")
        ),
    );
    let ts = tables(&d);
    assert_eq!(ts[0].rows[0].cells.len(), 3);
    for p in [
        &d.document.sections()[0].paragraphs[0],
        &ts[0].rows[0].cells[1].paragraphs[0],
        &ts[0].rows[0].cells[2].paragraphs[0],
    ] {
        let s = &d.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(s.list, None);
        assert_eq!((s.space_before.as_i32(), s.space_after.as_i32()), (0, 0));
        assert_eq!(p.style_ref, None);
    }
}
// 이것을 실패시키는 것: 생략 열/깊이/표 위치 한도를 펼친 뒤 검사.
#[test]
fn table_limits_reject_before_expansion() {
    for body in [
        "<w:tbl><w:tr><w:trPr><w:gridBefore w:val='1048577'/></w:trPr></w:tr></w:tbl>".to_owned(),
        format!(
            "<w:tbl>{}</w:tbl>",
            format!("<w:tr>{}</w:tr>", cell("<w:gridSpan w:val='65535'/>", "A")).repeat(17)
        ),
    ] {
        assert!(matches!(decode_docx(&package(&body, &[])), Err(crate::DocxError::Limit { .. })));
    }
    for depth in [32, 33] {
        let body = format!(
            "{}<w:p/>{}",
            "<w:tbl><w:tr><w:tc>".repeat(depth),
            "</w:tc></w:tr></w:tbl>".repeat(depth)
        );
        let result = decode_docx(&package(&body, &[]));
        if depth == 32 {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(matches!(
                result,
                Err(crate::DocxError::Limit { resource: "table nesting", .. })
            ));
        }
    }
}
// 이것을 실패시키는 것: 중첩 폭에서 여백을 무시하거나 절대 폭 범위를 clamp 뒤 검사.
#[test]
fn structural_width_and_margin_errors() {
    for prop in [
        "<w:tblW w:type='dxa' w:w='20000001'/>",
        "<w:tblW w:type='dxa' w:w='-1'/>",
        "<w:tblW w:type='dxa'/>",
        "<w:tblCellMar><w:left w:type='dxa' w:w='20000000'/></w:tblCellMar>",
    ] {
        assert!(decode_docx(&package(&small_table(prop, &cell("", "A")), &[])).is_err(), "{prop}");
    }
    let d = read("", &small_table("<w:tblW w:type='dxa' w:w='20000000'/>", &cell("", "A")));
    assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 48190);
}
// 이것을 실패시키는 것: 앞 행의 다른 구간/끝난 anchor를 continue로 연결.
#[test]
fn merge_mismatches_and_restarts() {
    let body = format!(
        "<w:tbl><w:tr>{}{}</w:tr><w:tr>{}</w:tr></w:tbl>",
        cell("", "A"),
        cell("<w:vMerge w:val='restart'/>", "B"),
        cell("<w:gridSpan w:val='2'/><w:vMerge/>", "C")
    );
    assert!(decode_docx(&package(&body, &[])).is_err());
    let body = format!(
        "<w:tbl><w:tr>{}</w:tr><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
        cell("<w:vMerge w:val='restart'/>", "A"),
        cell("", "B"),
        cell("<w:vMerge/>", "C")
    );
    let d = read("", &body);
    assert_eq!(
        tables(&d)[0].rows.iter().map(|r| r.cells[0].row_span).collect::<Vec<_>>(),
        [1, 1, 1]
    );
    assert!(d.warnings.iter().any(|w| w.loss() == "vertical_merge"));
}
// 이것을 실패시키는 것: 조건부 스타일/미지원 선의 무음 제외 또는 스타일 이름 하드코딩.
#[test]
fn table_style_losses_and_direct_precedence() {
    let styles="<w:style w:type='table' w:default='1' w:styleId='odd'><w:tblPr><w:tblCellMar><w:left w:w='0'/><w:right w:w='0'/></w:tblCellMar><w:tblBorders><w:left w:val='double'/></w:tblBorders><w:tblW w:type='dxa' w:w='20'/></w:tblPr><w:tblStylePr w:type='firstRow'><w:tcPr><w:shd w:fill='FF0000'/></w:tcPr></w:tblStylePr></w:style>";
    let d = read(
        styles,
        &small_table(
            "<w:tblW w:type='auto' w:w='0'/><w:tblBorders><w:left w:val='none'/></w:tblBorders>",
            &cell("", "A"),
        ),
    );
    assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 48190);
    assert_eq!(
        d.registry
            .border_fill(tables(&d)[0].rows[0].cells[0].border_fill_id.unwrap())
            .unwrap()
            .border
            .left
            .line_type,
        BorderLineType::None
    );
    for loss in ["conditional_table_style", "table_border"] {
        assert!(d.warnings.iter().any(|w| w.loss() == loss));
    }
    let d = read(styles, &small_table("<w:tblStyle w:val='missing'/>", &cell("", "A")));
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_STYLE_REFERENCE_LOST"));
    assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 100);
}
// 이것을 실패시키는 것: 속성/선 종류/색 손실을 무음 처리하거나 cell 안 섹션으로 다음 구역 폭 변경.
#[test]
fn table_property_warning_matrix() {
    for prop in [
        "<w:tblBorders><w:left w:val='dashed'/></w:tblBorders>",
        "<w:tblBorders><w:right w:val='single' w:color='FF0000'/></w:tblBorders>",
        "<w:tblBorders><w:top w:val='single' w:sz='4'/></w:tblBorders>",
    ] {
        let d = read("", &small_table(prop, &cell("", "A")));
        assert!(d.warnings.iter().any(|w| w.loss() == "table_border"));
    }
    for prop in [
        "<w:tblBorders><w:start w:val='single'/></w:tblBorders>",
        "<w:tblCellMar><w:left w:type='pct' w:w='10'/></w:tblCellMar>",
        "<w:tblW w:type='auto' w:w='0' w:unknown='1'/>",
    ] {
        let d = read("", &small_table(prop, &cell("", "A")));
        assert!(d.warnings.iter().any(|w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE"));
    }
    for body in [
        "<w:tbl><w:tblGrid/><w:tblGrid/></w:tbl>",
        "<w:tbl><w:tr/><w:tblGrid/></w:tbl>",
        "<w:p><w:tbl/></w:p>",
        "<w:tcPr/>",
    ] {
        assert!(decode_docx(&package(body, &[])).is_err());
    }
}
// 이것을 실패시키는 것: 셀 경계에서 후행 나누기를 다음 셀 또는 표 밖으로 이동.
#[test]
fn cell_and_table_boundaries_keep_trailing_breaks_local() {
    let d=read("","<w:tbl><w:tr><w:tc><w:p><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>C</w:t></w:r></w:p>");
    let ts = tables(&d);
    let cells = &ts[0].rows[0].cells;
    assert_eq!(
        cells[0].paragraphs.iter().map(|p| (p.text_content(), p.page_break)).collect::<Vec<_>>(),
        [("A".into(), false), ("".into(), true)]
    );
    assert!(!cells[1].paragraphs[0].page_break);
    assert!(!d.document.sections()[0].paragraphs[1].page_break);
}
// 이것을 실패시키는 것: tblGrid 대신 충돌하는 tcW 비율을 사용하거나 tcW 추정을 생략.
#[test]
fn nonuniform_grid_wins_cell_preferences() {
    let row = cell("<w:tcW w:type='dxa' w:w='20'/>", "A").repeat(3);
    let d=read(ZERO_MARGIN_STYLE,&format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='60'/></w:tblPr><w:tblGrid><w:gridCol w:w='10'/><w:gridCol w:w='20'/><w:gridCol w:w='30'/></w:tblGrid><w:tr>{row}</w:tr></w:tbl>"));
    assert_eq!(
        tables(&d)[0].rows[0].cells.iter().map(|c| c.width.as_i32()).collect::<Vec<_>>(),
        [50, 100, 150]
    );
    let row = [20, 40, 60]
        .iter()
        .map(|n| cell(&format!("<w:tcW w:type='dxa' w:w='{n}'/>"), "A"))
        .collect::<String>();
    let d = read(ZERO_MARGIN_STYLE, &small_table("<w:tblW w:type='dxa' w:w='120'/>", &row));
    assert_eq!(
        tables(&d)[0].rows[0].cells.iter().map(|c| c.width.as_i32()).collect::<Vec<_>>(),
        [100, 200, 300]
    );
}
// 이것을 실패시키는 것: 표/셀 문단에서 표 스타일 문맥 적용 누락.
#[test]
fn table_paragraph_context_precedes_direct_properties() {
    let styles="<w:style w:type='table' w:default='1' w:styleId='layout'><w:pPr><w:ind w:left='240'/></w:pPr></w:style>";
    let d = read(
        styles,
        &format!(
            "{}<w:p/>",
            small_table(
                "",
                &format!(
                    "{}<w:tc><w:p><w:pPr><w:ind w:left='600'/></w:pPr></w:p></w:tc>",
                    cell("", "A")
                )
            )
        ),
    );
    let ts = tables(&d);
    let cells = &ts[0].rows[0].cells;
    assert_eq!(
        cells
            .iter()
            .map(|c| d.registry.para_shapes[c.paragraphs[0].para_shape_id.get()]
                .indent_left
                .as_i32())
            .collect::<Vec<_>>(),
        [1200, 3000]
    );
    assert_eq!(
        d.registry.para_shapes[d.document.sections()[0].paragraphs[1].para_shape_id.get()]
            .indent_left
            .as_i32(),
        0
    );
}
// 이것을 실패시키는 것: 동일 표의 예산을 매 문단마다 다시 누적.
#[test]
fn position_budget_counts_growth_once() {
    let body = format!(
        "<w:tbl>{}</w:tbl>",
        format!("<w:tr>{}</w:tr>", cell("", "A").repeat(100)).repeat(100)
    );
    let d = read(ZERO_MARGIN_STYLE, &body);
    assert_eq!(TableGrid::from_table(&tables(&d)[0]).unwrap().dimensions(), (100, 100));
}
// 이것을 실패시키는 것: continuation의 객체 제외를 CONTENT_LOST로 분류.
#[test]
fn continuation_objects_have_object_omission() {
    let body=format!("<w:tbl><w:tr>{}</w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:drawing/></w:r></w:p></w:tc></w:tr></w:tbl>",cell("<w:vMerge w:val='restart'/>","A"));
    let d = read("", &body);
    assert!(d.warnings.iter().any(|w| w.code() == "DOCX_CONTENT_OMITTED"
        && w.loss() == "image_shape"
        && w.count() == 1));
}
// 이것을 실패시키는 것: 안쪽/바깥쪽 면 배분을 반대로 하거나 병합 면의 충돌 전파를 끊음.
#[test]
fn outer_and_inner_borders_are_distributed_per_face() {
    let border="<w:tblBorders><w:left w:val='single'/><w:right w:val='single'/><w:top w:val='single'/><w:bottom w:val='single'/><w:insideH w:val='none'/><w:insideV w:val='none'/></w:tblBorders>";
    let d = read(
        "",
        &format!(
            "<w:tbl><w:tblPr>{border}</w:tblPr><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",
            cell("", "A").repeat(2),
            cell("", "B").repeat(2)
        ),
    );
    let ts = tables(&d);
    let faces = ts[0]
        .rows
        .iter()
        .flat_map(|r| r.cells.iter())
        .map(|c| {
            let b = &d.registry.border_fill(c.border_fill_id.unwrap()).unwrap().border;
            [b.left.line_type, b.right.line_type, b.top.line_type, b.bottom.line_type]
                .map(|l| l == BorderLineType::Solid)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        faces,
        [
            [true, false, true, false],
            [false, true, true, false],
            [true, false, false, true],
            [false, true, false, true]
        ]
    );
    assert!(!d.warnings.iter().any(|w| w.loss() == "border_conflict"));
    let d=read("",&format!("<w:tbl><w:tblPr><w:tblBorders><w:insideH w:val='single'/></w:tblBorders></w:tblPr><w:tr>{}</w:tr><w:tr>{}{}</w:tr></w:tbl>",cell("<w:gridSpan w:val='2'/>","A"),cell("<w:tcBorders><w:top w:val='none'/></w:tcBorders>","B"),cell("","C")));
    let ts = tables(&d);
    assert_eq!(
        d.registry
            .border_fill(ts[0].rows[0].cells[0].border_fill_id.unwrap())
            .unwrap()
            .border
            .bottom
            .line_type,
        BorderLineType::None
    );
    for c in &ts[0].rows[1].cells {
        assert_eq!(
            d.registry.border_fill(c.border_fill_id.unwrap()).unwrap().border.top.line_type,
            BorderLineType::None
        );
    }
}

// 이것을 실패시키는 것: 스타일 셀 배경이나 셀 내부 선의 제외를 진단하지 않음.
#[test]
fn style_cell_background_and_internal_cell_borders_are_diagnosed() {
    let d = read(
        "<w:style w:type='table' w:default='1' w:styleId='S'><w:tcPr><w:shd w:fill='FF0000'/></w:tcPr></w:style>",
        &format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell("<w:tcBorders><w:insideH w:val='single'/><w:insideV w:val='single'/></w:tcBorders>", "A")),
    );
    assert!(d
        .warnings
        .iter()
        .any(|w| w.code() == "DOCX_CONTENT_LOST" && w.loss() == "cell_background"));
    assert!(d.warnings.iter().any(|w| w.loss() == "cell_internal_border"));
    assert!(tables(&d)[0].rows[0].cells[0].background.is_none());
}

// These cases isolate structural diagnostics from independently tested format defaults.
fn structure(body: &str) -> DocxDecoded {
    let mut d = read("", body);
    d.warnings.retain(|w| {
        w.code() != "DOCX_FORMAT_DEFAULTED" && !matches!(w.loss(), "page_size" | "page_margins")
    });
    d
}
fn fixed_table(content: &str) -> String {
    format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='1000'/><w:gridCol w:w='1000'/></w:tblGrid>{content}</w:tbl>")
}
fn at_table_level(level: &str, wrap: impl Fn(&str) -> String) -> String {
    let a = cell("", "A");
    let b = cell("", "B");
    let row = match level {
        "tbl" => wrap(&format!("<w:tr>{a}{b}</w:tr>")),
        "tr" => format!("<w:tr>{}{b}</w:tr>", wrap(&a)),
        "tc" => format!(
            "<w:tr><w:tc>{}</w:tc>{b}</w:tr>",
            wrap("<w:tcPr/><w:p><w:r><w:t>A</w:t></w:r></w:p>")
        ),
        _ => unreachable!(),
    };
    fixed_table(&row)
}
fn assert_pair(d: &DocxDecoded) {
    let ts = tables(d);
    assert_eq!(ts.len(), 1);
    let t = &ts[0];
    let grid = TableGrid::from_table(t).unwrap();
    assert_eq!(grid.dimensions(), (1, 2));
    assert_eq!(
        grid.iter_anchors()
            .map(|c| (c.anchor.row, c.anchor.col, c.row_span, c.col_span))
            .collect::<Vec<_>>(),
        [(0, 0, 1, 1), (0, 1, 1, 1)]
    );
    assert_eq!(
        t.rows[0].cells.iter().map(|c| c.paragraphs[0].text_content()).collect::<Vec<_>>(),
        ["A", "B"]
    );
}
// 이것을 실패시키는 것: 표 경로가 SDT를 차단하거나 래퍼를 구조 부모로 삼음, sdtPr 본문 노출.
#[test]
fn structured_wrappers_preserve_table_coordinates_and_text() {
    for level in ["tbl", "tr", "tc"] {
        for (open, close, loss) in [
            (
                "<w:sdt><w:sdtPr><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:sdtPr><w:sdtContent>",
                "</w:sdtContent></w:sdt>",
                "content_control_function",
            ),
            (
                "<w:customXml w:element='item'><w:customXmlPr><w:p/></w:customXmlPr>",
                "</w:customXml>",
                "structured_wrapper",
            ),
        ] {
            let d = structure(&at_table_level(level, |s| format!("{open}{s}{close}")));
            assert_pair(&d);
            assert_eq!(d.warnings.len(), 1, "{level}: {:?}", d.warnings);
            assert_eq!(d.warnings[0].loss(), loss);
            assert_eq!(d.warnings[0].count(), if loss == "structured_wrapper" { 2 } else { 1 });
        }
    }
    let d = structure(&at_table_level("tbl", |s| {
        format!("<w:sdt><w:sdtContent><w:customXml>{s}</w:customXml></w:sdtContent></w:sdt>")
    }));
    assert_pair(&d);
    assert_eq!(d.warnings.len(), 2);
}
// 이것을 실패시키는 것: 범위 표식을 표 속성/내용 손실로 경고하거나 뒤 셀을 차단.
#[test]
fn range_markers_at_table_levels_are_silent() {
    for level in ["tbl", "tr", "tc"] {
        for marker in ["bookmarkStart", "bookmarkEnd", "proofErr", "permStart", "permEnd"] {
            let d = structure(&at_table_level(level, |s| {
                format!("<w:{marker} w:id='1'/>{s}<w:{marker} w:id='1'/>")
            }));
            assert_pair(&d);
            assert!(d.warnings.is_empty(), "{level}/{marker}: {:?}", d.warnings);
        }
    }
    let d = structure(&fixed_table(&format!(
        "<w:tr>{}{}<w:bookmarkEnd w:id='1'/></w:tr>",
        cell("", "A"),
        cell("", "B")
    )));
    assert_pair(&d);
    assert!(d.warnings.is_empty());
}
// 이것을 실패시키는 것: 본문과 달리 이동 변경 추적 범위를 표 안에서 허용.
#[test]
fn move_ranges_in_tables_retain_revision_rejection() {
    for level in ["tbl", "tr", "tc"] {
        for marker in
            ["moveFromRangeStart", "moveFromRangeEnd", "moveToRangeStart", "moveToRangeEnd"]
        {
            let body = at_table_level(level, |s| format!("{s}<w:{marker} w:id='1'/>"));
            assert!(matches!(
                decode_docx(&package(&body, &[])),
                Err(crate::DocxError::TrackedChange { .. })
            ));
        }
    }
}
// 이것을 실패시키는 것: tblPrEx를 속성 경고로 분류하거나 그 테두리/여백을 overlay, 미지 요소를 래퍼처럼 통과.
#[test]
fn table_exceptions_and_unknown_children_report_content_loss() {
    for level in ["tbl", "tr", "tc"] {
        for (element, code, loss) in [
            ("tblPrEx", "DOCX_CONTENT_LOST", "table_property_exceptions"),
            ("unknown", "DOCX_CONTENT_OMITTED", "unknown_element"),
        ] {
            let d = structure(&at_table_level(level, |s| {
                format!("<w:{element}><w:tblBorders><w:left w:val='single'/></w:tblBorders><w:tblCellMar><w:left w:w='10'/></w:tblCellMar><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:{element}>{s}")
            }));
            assert_pair(&d);
            assert_eq!(d.warnings.len(), 1);
            assert_eq!(
                (d.warnings[0].code(), d.warnings[0].loss(), d.warnings[0].count()),
                (code, loss, 1)
            );
            let t = &tables(&d)[0];
            assert_eq!(t.in_margin.unwrap().left.as_i32(), 540);
            assert_eq!(t.rows[0].cells[0].margin.unwrap().left.as_i32(), 540);
            assert_eq!(
                d.registry
                    .border_fill(t.rows[0].cells[0].border_fill_id.unwrap())
                    .unwrap()
                    .border
                    .left
                    .line_type,
                BorderLineType::None
            );
        }
    }
}
// 이것을 실패시키는 것: auto의 선택적 val에 손실 경고 발생, auto의 명시된 잘못된 높이 허용.
#[test]
fn optional_row_height_is_silent_for_auto_and_validates_explicit_values() {
    for height in ["", " w:val='20'"] {
        let d = structure(&fixed_table(&format!(
            "<w:tr><w:trPr><w:trHeight w:hRule='auto'{height}/></w:trPr>{}{}</w:tr>",
            cell("", "A"),
            cell("", "B")
        )));
        assert_pair(&d);
        assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    }
    for height in ["oops", "-1", "20000001"] {
        for rule in ["", " w:hRule='auto'", " w:hRule='atLeast'", " w:hRule='exact'"] {
            let body = fixed_table(&format!(
                "<w:tr><w:trPr><w:trHeight w:val='{height}'{rule}/></w:trPr>{}{}</w:tr>",
                cell("", "A"),
                cell("", "B")
            ));
            assert!(decode_docx(&package(&body, &[])).is_err(), "{rule}/{height}");
        }
    }
}
// 이것을 실패시키는 것: 버릴 상대 폭을 구조 숫자로 파싱, 절대 폭 검증 완화, type 생략을 상대 폭으로 처리.
#[test]
fn non_absolute_width_ignores_numeric_syntax() {
    for ty in [" w:type='pct'", " w:type='auto'", " w:type='nil'"] {
        for value in ["100%", "oops", "-1", "20000001"] {
            for cell_width in [false, true] {
                let (pr, cells) = if cell_width {
                    (
                        "<w:tblW w:type='dxa' w:w='2000'/>".to_string(),
                        cell(&format!("<w:tcW{ty} w:w='{value}'/>"), "A"),
                    )
                } else {
                    (format!("<w:tblW{ty} w:w='{value}'/>"), cell("", "A"))
                };
                let d = structure(&small_table(&pr, &cells));
                assert_eq!(tables(&d)[0].rows[0].cells[0].paragraphs[0].text_content(), "A");
                assert!(d.warnings.iter().any(|w| w.code() == "DOCX_TABLE_WIDTH_APPROXIMATED"
                    && w.action() == "use_container"));
            }
        }
    }
    for value in ["100%", "oops", "-1", "0", "20000001"] {
        for name in ["tblW", "tcW"] {
            let prop = format!("<w:{name} w:type='dxa' w:w='{value}'/>");
            let body = if name == "tblW" {
                small_table(&prop, &cell("", "A"))
            } else {
                small_table("", &cell(&prop, "A"))
            };
            assert!(decode_docx(&package(&body, &[])).is_err());
        }
    }
}
// 이것을 실패시키는 것: 미지정 면의 Word 기본 여백 누락, 일부 면/상속/셀 override 무시.
#[test]
fn absent_and_partial_margins_keep_core_contract() {
    let margins = |m: Option<hwpforge_core::table::TableMargin>| {
        m.map(|m| [m.left.as_i32(), m.right.as_i32(), m.top.as_i32(), m.bottom.as_i32()])
    };
    for (style, table_pr, cell_pr, expected_table, expected_cell, nested_width) in [
        ("", "", "", Some([540,540,0,0]), Some([540,540,0,0]), 3920),
        ("", "<w:tblCellMar><w:left w:w='20'/></w:tblCellMar>", "", Some([100,540,0,0]), Some([100,540,0,0]), 4360),
        ("", "", "<w:tcMar><w:right w:w='10'/></w:tcMar>", Some([540,540,0,0]), Some([540,50,0,0]), 4410),
        ("<w:style w:type='table' w:default='1' w:styleId='base'><w:tblPr><w:tblCellMar><w:left w:w='20'/><w:top w:w='6'/></w:tblCellMar></w:tblPr></w:style>", "<w:tblCellMar><w:right w:w='10'/></w:tblCellMar>", "<w:tcMar><w:left w:w='0'/></w:tcMar>", Some([100,50,30,0]), Some([0,50,30,0]), 4950),
    ] {
        let nested = small_table("<w:tblW w:type='auto'/>", &cell("", "N"));
        let body = small_table(&format!("<w:tblW w:type='dxa' w:w='1000'/>{table_pr}"), &format!("<w:tc><w:tcPr>{cell_pr}</w:tcPr>{nested}</w:tc>"));
        let d = read(style, &body);
        let ts = tables(&d);
        assert_eq!(margins(ts[0].in_margin), expected_table);
        assert_eq!(margins(ts[0].rows[0].cells[0].margin), expected_cell);
        assert_eq!(ts[1].width.unwrap().as_i32(), nested_width);
    }
}
// 이것을 실패시키는 것: space=0으로 근사 경고 발생, 0 아닌 space 경고 제거, themeColor 이중 경고.
#[test]
fn border_spacing_and_theme_have_one_loss_diagnostic() {
    for (extra, count) in [
        ("", 0),
        (" w:space='0'", 0),
        (" w:space='1'", 1),
        (" w:themeColor='accent1'", 1),
        (" w:space='0' w:themeColor='accent1'", 1),
    ] {
        let body = fixed_table(&format!(
            "<w:tr>{}{}</w:tr>",
            cell(&format!("<w:tcBorders><w:left w:val='single'{extra}/></w:tcBorders>"), "A"),
            cell("", "B")
        ));
        let d = structure(&body);
        assert_pair(&d);
        assert_eq!(d.warnings.len(), count, "{extra}: {:?}", d.warnings);
        if count != 0 {
            assert_eq!(d.warnings[0].code(), "DOCX_TABLE_BORDER_APPROXIMATED");
            assert_eq!(d.warnings[0].count(), 1);
        }
    }
}

fn selection_wrapper(wrapper: &str, body: &str) -> String {
    if let Some((outer, inner)) = wrapper.split_once('/') {
        return selection_wrapper(outer, &selection_wrapper(inner, body));
    }
    match wrapper {
        "sdt" => format!("<w:sdt><w:sdtPr><w:p><w:r><w:t>hidden</w:t></w:r></w:p></w:sdtPr><w:sdtContent>{body}</w:sdtContent></w:sdt>"),
        "custom" => format!("<w:customXml>{body}</w:customXml>"),
        "fallback" => format!("<mc:AlternateContent xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'><mc:Fallback>{body}</mc:Fallback></mc:AlternateContent>"),
        "choice" => format!("<mc:AlternateContent xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'><mc:Choice Requires='w'>{body}</mc:Choice><mc:Fallback/></mc:AlternateContent>"),
        "excluded" => format!("<w:object>{body}</w:object>"),
        _ => unreachable!(),
    }
}
fn selection_case(position: &str, wrapper: &str) {
    if wrapper == "double" {
        for outer in ["sdt", "custom", "fallback", "choice", "excluded"] {
            for inner in ["sdt", "custom", "fallback", "choice", "excluded"] {
                selection_case(position, &format!("{outer}/{inner}"));
            }
        }
        return;
    }
    let wrap = |body: &str| selection_wrapper(wrapper, body);
    let a = cell("", "A");
    let b = cell("", "B");
    let row = format!("<w:tr>{a}</w:tr>");
    let body = match position {
        "body" => {
            format!("{}<w:tbl><w:tr>{b}</w:tr></w:tbl>", wrap(&format!("<w:tbl>{row}</w:tbl>")))
        }
        "tbl" => format!("<w:tbl>{}<w:tr>{b}</w:tr></w:tbl>", wrap(&row)),
        "tr" => format!("<w:tbl><w:tr>{}{b}</w:tr></w:tbl>", wrap(&a)),
        "tc" => format!(
            "<w:tbl><w:tr><w:tc>{}<w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr></w:tbl>",
            wrap("<w:tcPr/><w:p><w:r><w:t>A</w:t></w:r></w:p>")
        ),
        _ => unreachable!(),
    };
    let d = read("", &body);
    let ts = tables(&d);
    let text: String = ts
        .iter()
        .flat_map(|t| &t.rows)
        .flat_map(|r| &r.cells)
        .flat_map(|c| &c.paragraphs)
        .map(|p| p.text_content())
        .collect();
    let selected = !wrapper.split('/').any(|w| matches!(w, "choice" | "excluded"));
    assert_eq!(text, if selected { "AB" } else { "B" }, "{position}/{wrapper}");
    let coords: Vec<_> = ts
        .iter()
        .flat_map(|t| {
            TableGrid::from_table(t)
                .unwrap()
                .iter_anchors()
                .map(|c| (c.anchor.row, c.anchor.col, c.row_span, c.col_span))
                .collect::<Vec<_>>()
        })
        .collect();
    let expected = match (position, selected) {
        ("body", true) => vec![(0, 0, 1, 1), (0, 0, 1, 1)],
        ("tbl", true) => vec![(0, 0, 1, 1), (1, 0, 1, 1)],
        ("tr", true) => vec![(0, 0, 1, 1), (0, 1, 1, 1)],
        _ => vec![(0, 0, 1, 1)],
    };
    assert_eq!(coords, expected);
    let mut selections = 0;
    for w in wrapper.split('/') {
        selections += u64::from(matches!(w, "fallback" | "choice"));
        if matches!(w, "choice" | "excluded") {
            break;
        }
    }
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.code() == "DOCX_ALTERNATE_CONTENT_SELECTED")
            .map(|w| w.count())
            .sum::<u64>(),
        selections
    );
}
macro_rules! selection_tests {
    ($($name:ident: $position:literal, $wrapper:literal;)*) => {$ (
        // 이것을 실패시키는 것: 선택 프레임의 구조 투명성 제거 또는 제외 프레임까지 건너뛰기.
        #[test]
        fn $name() { selection_case($position, $wrapper); }
    )*};
}
selection_tests! {
    selection_body_sdt: "body", "sdt";
    selection_body_custom: "body", "custom";
    selection_body_fallback: "body", "fallback";
    selection_body_choice: "body", "choice";
    selection_body_excluded: "body", "excluded";
    selection_body_double: "body", "double";
    selection_tbl_sdt: "tbl", "sdt";
    selection_tbl_custom: "tbl", "custom";
    selection_tbl_fallback: "tbl", "fallback";
    selection_tbl_choice: "tbl", "choice";
    selection_tbl_excluded: "tbl", "excluded";
    selection_tbl_double: "tbl", "double";
    selection_tr_sdt: "tr", "sdt";
    selection_tr_custom: "tr", "custom";
    selection_tr_fallback: "tr", "fallback";
    selection_tr_choice: "tr", "choice";
    selection_tr_excluded: "tr", "excluded";
    selection_tr_double: "tr", "double";
    selection_tc_sdt: "tc", "sdt";
    selection_tc_custom: "tc", "custom";
    selection_tc_fallback: "tc", "fallback";
    selection_tc_choice: "tc", "choice";
    selection_tc_excluded: "tc", "excluded";
    selection_tc_double: "tc", "double";
}

fn height_cell(extra: &str, paragraphs: &str) -> String {
    format!("<w:tc><w:tcPr>{extra}</w:tcPr>{paragraphs}</w:tc>")
}
fn height_paragraph(rule: &str, line: u32, size: u32, text: &str) -> String {
    format!("<w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='{rule}'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='{size}'/></w:rPr><w:t>{text}</w:t></w:r></w:p>")
}
// 이것을 실패시키는 것: 최소 높이 미지정, auto 배수/자연 높이/여백 누락, atLeast와 exact 혼동.
#[test]
fn row_minimum_reuses_word_line_metrics_and_explicit_height() {
    // 1100 * (2229 + 495) / 2048 * 1.3 = 1902.01171875 -> 1902.
    for (row_pr, line_rule, line, expected) in [
        ("", "auto", 240, 1902),
        ("", "auto", 241, 1910),
        ("", "auto", 480, 3804),
        ("", "exact", 200, 1000),
        ("", "atLeast", 200, 1902),
        ("", "atLeast", 600, 3000),
        ("<w:trHeight w:val='600' w:hRule='atLeast'/>", "auto", 240, 3000),
        ("<w:trHeight w:val='200' w:hRule='atLeast'/>", "auto", 240, 1902),
        ("<w:trHeight w:val='600'/>", "auto", 240, 3000),
        ("<w:trHeight w:val='200' w:hRule='exact'/>", "auto", 240, 1000),
    ] {
        let c = height_cell("", &height_paragraph(line_rule, line, 22, "한글"));
        let d = read("", &format!("<w:tbl><w:tr><w:trPr>{row_pr}</w:trPr>{c}</w:tr></w:tbl>"));
        assert_eq!(
            tables(&d)[0].rows[0].height.unwrap().as_i32(),
            expected,
            "{row_pr}/{line_rule}"
        );
        assert_eq!(
            d.warnings.iter().filter(|w| w.loss() == "row_height").count(),
            usize::from(row_pr.contains("exact"))
        );
    }
    let c = height_cell(
        "<w:tcMar><w:top w:w='20'/><w:bottom w:w='30'/></w:tcMar>",
        &height_paragraph("auto", 240, 22, "한글"),
    );
    let d = read(
        "",
        &format!(
            "<w:tbl><w:tr>{c}{}</w:tr></w:tbl>",
            height_cell("", &height_paragraph("auto", 240, 20, "한글"))
        ),
    );
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 2152);
}
// 이것을 실패시키는 것: 병합 anchor의 문단 높이를 시작 행 또는 다음 행에 합산.
#[test]
fn spanning_content_does_not_enlarge_individual_rows() {
    let first = height_paragraph("auto", 240, 22, "한글</w:t><w:br/><w:t>");
    let first = first.replace(
        "</w:r></w:p>",
        "</w:r><w:r><w:rPr><w:sz w:val='80'/></w:rPr><w:t>큰</w:t></w:r></w:p>",
    );
    let a = height_cell(
        "<w:vMerge w:val='restart'/>",
        &format!("{first}{}", height_paragraph("auto", 240, 80, "큰")),
    );
    let b = height_cell("", &height_paragraph("exact", 100, 22, "B"));
    let c = cell("<w:vMerge/>", "lost");
    let d = read("", &format!("<w:tbl><w:tr>{a}{b}</w:tr><w:tr>{c}{b}</w:tr></w:tbl>"));
    assert_eq!(
        tables(&d)[0].rows.iter().map(|r| r.height.unwrap().as_i32()).collect::<Vec<_>>(),
        [500, 500]
    );
}
// 이것을 실패시키는 것: 병합 영역 max에서 내용 최소를 빼거나 시작 행에 내용 높이를 부과.
#[test]
fn merged_content_minimum_without_row_heights() {
    let p = height_paragraph("auto", 240, 22, "가</w:t><w:br/><w:t>나</w:t><w:br/><w:t>다")
        .replace("<w:spacing ", "<w:spacing w:before='0' w:after='0' ");
    let anchor = height_cell("<w:vMerge w:val='restart'/>", &p);
    let continuation = height_cell("<w:vMerge/>", "<w:p/>");
    for (row_pr, row_height, merged_height) in
        [("", 0, 5706), ("<w:trPr><w:trHeight w:val='600'/></w:trPr>", 3000, 6000)]
    {
        let d = read(
            "",
            &format!(
                "<w:tbl><w:tr>{row_pr}{anchor}</w:tr><w:tr>{row_pr}{continuation}</w:tr></w:tbl>"
            ),
        );
        let ts = tables(&d);
        assert_eq!(ts[0].rows[0].cells[0].row_span, 2);
        assert_eq!(ts[0].rows[0].cells[0].height.unwrap().as_i32(), merged_height);
        assert!(ts[0].rows[1].cells.is_empty());
        for row in &ts[0].rows {
            assert_eq!(row.height.unwrap().as_i32(), row_height);
        }
    }
    let d = read("", &format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", height_cell("", &p)));
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 5706);
}

// 이것을 실패시키는 것: 병합 영역 max에서 내용 최소를 빼거나 같은 시작 행의 일반 셀 높이를 누락.
#[test]
fn merged_content_minimum_preserves_ordinary_cell_height() {
    let p = height_paragraph("auto", 240, 22, "가</w:t><w:br/><w:t>나</w:t><w:br/><w:t>다")
        .replace("<w:spacing ", "<w:spacing w:before='0' w:after='0' ");
    let anchor = height_cell("<w:vMerge w:val='restart'/>", &p);
    let continuation = height_cell("<w:vMerge/>", "<w:p/>");
    let ordinary = height_cell("", &height_paragraph("auto", 240, 22, "가"));
    let d = read(
        "",
        &format!(
            "<w:tbl><w:tr>{anchor}{ordinary}</w:tr><w:tr>{continuation}{ordinary}</w:tr></w:tbl>"
        ),
    );
    let ts = tables(&d);
    assert_eq!(ts[0].rows[0].cells[0].height.unwrap().as_i32(), 5706);
    assert_eq!(ts[0].rows[0].cells[1].height.unwrap().as_i32(), 1902);
    for row in &ts[0].rows {
        assert_eq!(row.height.unwrap().as_i32(), 1902);
    }
    assert!(ts[0].rows[1].cells[0].height.is_none());
}

// 이것을 실패시키는 것: continuation bottom override 삭제, 부분 측면 충돌 무경고/실선 우선, 중간 bottom 유지.
#[test]
fn merged_perimeter_folds_continuation_borders() {
    for (middle, last, expected_bottom) in [
        ("none", "none", BorderLineType::None),
        ("none", "single", BorderLineType::Solid),
        ("none", "", BorderLineType::Solid),
    ] {
        let border =
            |v: &str| if v.is_empty() { String::new() } else { format!("<w:bottom w:val='{v}'/>") };
        let a=cell("<w:vMerge w:val='restart'/><w:tcBorders><w:left w:val='single'/><w:right w:val='single'/></w:tcBorders>","A");
        let b=cell(&format!("<w:vMerge/><w:tcBorders><w:left w:val='none'/><w:right w:val='none'/>{}</w:tcBorders>",border(middle)),"");
        let c = cell(&format!("<w:vMerge/><w:tcBorders>{}</w:tcBorders>", border(last)), "");
        let d=read("",&format!("<w:tbl><w:tblPr><w:tblBorders><w:bottom w:val='single'/></w:tblBorders></w:tblPr><w:tr>{a}</w:tr><w:tr>{b}</w:tr><w:tr>{c}</w:tr></w:tbl>"));
        let ts = tables(&d);
        let fill = d.registry.border_fill(ts[0].rows[0].cells[0].border_fill_id.unwrap()).unwrap();
        assert_eq!(fill.border.bottom.line_type, expected_bottom);
        assert_eq!(
            (fill.border.left.line_type, fill.border.right.line_type),
            (BorderLineType::None, BorderLineType::None)
        );
        assert!(
            d.warnings
                .iter()
                .any(|w| w.code() == "DOCX_TABLE_BORDER_APPROXIMATED"
                    && w.loss() == "border_conflict")
        );
    }
    let d=read("",&format!("<w:tbl><w:tblPr><w:tblBorders><w:bottom w:val='single'/></w:tblBorders></w:tblPr><w:tr>{}</w:tr><w:tr>{}</w:tr></w:tbl>",cell("<w:vMerge w:val='restart'/>","A"),cell("<w:vMerge/><w:tcBorders><w:bottom w:val='none'/></w:tcBorders>","")));
    assert_eq!(
        d.registry
            .border_fill(tables(&d)[0].rows[0].cells[0].border_fill_id.unwrap())
            .unwrap()
            .border
            .bottom
            .line_type,
        BorderLineType::None
    );
}
// 이것을 실패시키는 것: 명시 세로 정렬 누락/교환 또는 미지정에 top 강제/경고 추가.
#[test]
fn explicit_vertical_alignment_preserves_default_choice() {
    use hwpforge_core::table::TableVerticalAlign::{Bottom, Center, Top};
    for (value, expected) in
        [("", None), ("top", Some(Top)), ("center", Some(Center)), ("bottom", Some(Bottom))]
    {
        let pr =
            if value.is_empty() { String::new() } else { format!("<w:vAlign w:val='{value}'/>") };
        let d = structure(&format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell(&pr, "A")));
        assert_eq!(tables(&d)[0].rows[0].cells[0].vertical_align, expected);
        assert!(!d.warnings.iter().any(|w| matches!(
            w.loss(),
            "table_attribute" | "table_property" | "cell_vertical_alignment"
        )));
    }
}
// 이것을 실패시키는 것: 제외 객체를 continuation에서 CONTENT_LOST로 분류하거나 지원 텍스트까지 OMITTED로 분류.
#[test]
fn object_omission_and_structural_content_loss_are_distinct() {
    for (node, loss) in [
        ("w:drawing", "image_shape"),
        ("w:pict", "image_shape"),
        ("w:object", "embedded_object"),
        ("w:altChunk", "embedded_object"),
        ("w:footnoteReference", "notes"),
        ("w:endnoteReference", "notes"),
        ("w:commentReference", "comments"),
        ("w:commentRangeStart", "comments"),
        ("w:commentRangeEnd", "comments"),
        ("w:sym", "symbol_character"),
        ("m:oMath", "math"),
    ] {
        let object = format!(
            "<w:p><w:r><{node}><w:p><w:r><w:t>hidden</w:t></w:r></w:p></{node}></w:r></w:p>"
        );
        let a = height_cell("<w:vMerge w:val='restart'/>", &object);
        let b =
            height_cell("<w:vMerge/>", &format!("{object}<w:p><w:r><w:t>lost</w:t></w:r></w:p>"));
        let d = read("", &format!("<w:tbl><w:tr>{a}</w:tr><w:tr>{b}</w:tr></w:tbl>"));
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_CONTENT_OMITTED" && w.loss() == loss)
                .map(|w| w.count())
                .sum::<u64>(),
            2
        );
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.code() == "DOCX_CONTENT_LOST" && w.loss() == "merge_continuation")
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
    }
}

// 이것을 실패시키는 것: 첫 줄의 라틴/빈 run/문단 끝 서식 선택 변경, 미지 글꼴에 자연 높이 합성, 반올림 대신 버림, 빈 첫 줄 무시.
#[test]
fn first_line_empty_mark_script_and_unknown_metrics() {
    for (paragraph, expected) in [
        (height_paragraph("auto",240,22,"ABC"),1463),
        (height_paragraph("auto",240,24,"한글"),2075),
        (height_paragraph("auto",240,22,"</w:t><w:br/><w:t>한글"),3804),
        (height_paragraph("auto",240,22,""),1902),
        (height_paragraph("auto",240,22,"한글").replace("맑은 고딕","Unknown"),1100),
        (height_paragraph("atLeast",100,22,"한글").replace("맑은 고딕","Unknown"),500),
        (height_paragraph("auto",0,22,"한글"),0),
        ("<w:p><w:pPr><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr></w:pPr></w:p>".into(),1902),
        ("<w:p><w:pPr><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='40'/></w:rPr></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t><w:br/></w:r></w:p>".into(),5360),
        (height_paragraph("auto",240,22,"한글").replace("<w:r>","<w:r/><w:r>"),1902),
    ] {
        let d=read("",&format!("<w:tbl><w:tr>{}</w:tr></w:tbl>",height_cell("",&paragraph)));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(),expected,"{paragraph}");
    }
}
// 이것을 실패시키는 것: 행 최소 높이 범위 검사 누락 또는 잘못된 세로 정렬을 무경고로 수락.
#[test]
fn row_minimum_overflow_and_unknown_alignment_are_diagnosed() {
    for c in [
        height_cell("", &height_paragraph("auto", 100000000, 22, "한글")),
        height_cell(
            "<w:tcMar><w:top w:w='20000000'/></w:tcMar>",
            &height_paragraph("auto", 240, 22, "한글"),
        ),
    ] {
        assert!(decode_docx(&package(&format!("<w:tbl><w:tr>{c}</w:tr></w:tbl>"), &[])).is_err());
    }
    let d =
        read("", &format!("<w:tbl><w:tr>{}</w:tr></w:tbl>", cell("<w:vAlign w:val='both'/>", "A")));
    assert_eq!(tables(&d)[0].rows[0].cells[0].vertical_align, None);
    assert_eq!(
        d.warnings
            .iter()
            .filter(
                |w| w.loss() == "cell_vertical_alignment" && w.code() == "DOCX_UNSUPPORTED_VALUE"
            )
            .count(),
        1
    );
}
// 이것을 실패시키는 것: 저장 결과/링크 본문 프레임만 구조 투명성에서 제외.
#[test]
fn result_and_link_wrappers_share_structural_transparency() {
    for wrapper in ["fldSimple", "hyperlink"] {
        for position in ["tbl", "tr", "tc"] {
            let d = read(
                "",
                &at_table_level(position, |body| format!("<w:{wrapper}>{body}</w:{wrapper}>")),
            );
            assert_pair(&d);
        }
    }
}

// 이것을 실패시키는 것: 중첩 표 보유용 합성 문단을 부모 셀의 첫 원본 문단으로 계산.
#[test]
fn nested_table_holder_does_not_supply_parent_first_line() {
    let nested = small_table("", &cell("", "N"));
    let outer = height_cell("", &format!("{nested}{}", height_paragraph("exact", 600, 22, "A")));
    let d = read("", &small_table("", &outer));
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 3000);
}

// 이것을 실패시키는 것: 병합 측면의 바깥/안쪽 상속을 무시하고 미지정을 없음으로 바꿈.
#[test]
fn merged_side_segments_use_table_border_inheritance() {
    for (left, right, inside, middle, expected) in [
        ("single", "single", "none", false, BorderLineType::Solid),
        ("none", "none", "single", true, BorderLineType::Solid),
        ("none", "none", "none", false, BorderLineType::None),
    ] {
        let borders=format!("<w:tblBorders><w:left w:val='{left}'/><w:right w:val='{right}'/><w:insideV w:val='{inside}'/></w:tblBorders>");
        let a = cell("<w:vMerge w:val='restart'/>", "A");
        let b = cell("<w:vMerge/>", "");
        let row = |c: &str| {
            if middle {
                format!("<w:tr>{}{}{}</w:tr>", cell("", "L"), c, cell("", "R"))
            } else {
                format!("<w:tr>{c}</w:tr>")
            }
        };
        let d =
            read("", &format!("<w:tbl><w:tblPr>{borders}</w:tblPr>{}{}</w:tbl>", row(&a), row(&b)));
        let ts = tables(&d);
        let c = &ts[0].rows[0].cells[usize::from(middle)];
        let border = &d.registry.border_fill(c.border_fill_id.unwrap()).unwrap().border;
        assert_eq!((border.left.line_type, border.right.line_type), (expected, expected));
        assert!(!d.warnings.iter().any(|w| w.loss() == "border_conflict"));
    }
}

// 이것을 실패시키는 것: auto를 atLeast와 합쳐 val을 최소 높이로 소비, exact/누락 val 경고 혼동.
#[test]
fn row_height_rules_keep_content_minima_and_exact_approximation() {
    // 맑은 고딕 11pt: 1100 * (2229 + 495) / 2048 * 1.3 = 1902.01171875 -> 1902.
    let c = height_cell("", &height_paragraph("auto", 240, 22, "한글"));
    // Complete source defaults let us assert all diagnostics without filtering.
    let defaults = "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/><w:color w:val='000000'/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:before='0' w:after='0' w:line='240' w:lineRule='auto'/><w:ind w:left='0' w:right='0' w:firstLine='0'/></w:pPr></w:pPrDefault></w:docDefaults>";
    let section = "<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>";
    let decode = |row_pr: &str| {
        read(defaults, &format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr>{row_pr}{c}</w:tr></w:tbl>{section}"))
    };
    let baseline = decode("");
    assert_eq!(tables(&baseline)[0].rows[0].height.unwrap().as_i32(), 1902);
    assert!(baseline.warnings.is_empty(), "{:?}", baseline.warnings);
    for (rule, value, expected, warnings) in [
        (" w:hRule='auto'", " w:val='2000'", 1902, 0),
        (" w:hRule='auto'", "", 1902, 0),
        (" w:hRule='auto'", " w:val='0'", 1902, 0),
        (" w:hRule='atLeast'", " w:val='2000'", 10000, 0),
        (" w:hRule='atLeast'", "", 1902, 1),
        (" w:hRule='atLeast'", " w:val='0'", 1902, 0),
        ("", " w:val='2000'", 10000, 0),
        ("", "", 1902, 1),
        ("", " w:val='0'", 1902, 0),
        (" w:hRule='exact'", " w:val='2000'", 10000, 1),
        (" w:hRule='exact'", "", 1902, 1),
        (" w:hRule='exact'", " w:val='0'", 0, 1),
    ] {
        let d = decode(&format!("<w:trPr><w:trHeight{value}{rule}/></w:trPr>"));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected, "{rule}/{value}");
        assert_eq!(d.warnings.len(), warnings, "{rule}/{value}: {:?}", d.warnings);
        if warnings > 0 {
            assert_eq!(
                (d.warnings[0].code(), d.warnings[0].loss(), d.warnings[0].action()),
                ("DOCX_CONTENT_LOST", "row_height", "use_default")
            );
            assert_eq!(
                d.warnings[0].message(),
                if rule.contains("exact") {
                    "Exact row height approximated as a content-expanding minimum"
                } else {
                    "Missing row height value or unsupported rule; using a minimum"
                }
            );
        }
    }
}
// 이것을 실패시키는 것: type 생략을 auto로 해석, 알 수 없는 type을 오류 또는 절대 폭으로 해석.
#[test]
fn absent_width_type_is_absolute_and_unknown_type_is_omitted() {
    for ty in ["", " w:type='dxa'"] {
        let d = read(
            "",
            &format!(
                "<w:tbl><w:tblPr><w:tblW{ty} w:w='1440'/></w:tblPr><w:tr>{}</w:tr></w:tbl>",
                cell("", "A")
            ),
        );
        assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 7200);
        let d = read(
            "",
            &format!(
                "<w:tbl><w:tblPr><w:tblW w:w='2000'/></w:tblPr><w:tr>{}{}</w:tr></w:tbl>",
                cell(&format!("<w:tcW{ty} w:w='500'/>"), "A"),
                cell(&format!("<w:tcW{ty} w:w='1500'/>"), "B")
            ),
        );
        assert_eq!(
            tables(&d)[0].rows[0].cells.iter().map(|c| c.width.as_i32()).collect::<Vec<_>>(),
            [2500, 7500]
        );
    }
    for name in ["tblW", "tcW"] {
        for value in ["oops", "0", "1440"] {
            let prop = format!("<w:{name} w:type='oops' w:w='{value}'/>");
            let body = if name == "tblW" {
                small_table(&prop, &cell("", "A"))
            } else {
                small_table("", &cell(&prop, "A"))
            };
            let d = read("", &body);
            assert_eq!(tables(&d)[0].width.unwrap().as_i32(), 48190);
            assert_eq!(
                d.warnings
                    .iter()
                    .filter(|w| w.code() == "DOCX_UNSUPPORTED_ATTRIBUTE"
                        && w.loss() == "table_width_type"
                        && w.action() == "omit")
                    .map(|w| w.count())
                    .sum::<u64>(),
                1
            );
        }
        for value in ["oops", "0", "-1", "20000001"] {
            let prop = format!("<w:{name} w:w='{value}'/>");
            let body = if name == "tblW" {
                small_table(&prop, &cell("", "A"))
            } else {
                small_table("", &cell(&prop, "A"))
            };
            assert!(decode_docx(&package(&body, &[])).is_err());
        }
    }
}
// 이것을 실패시키는 것: 기존 page/column 경계를 무시하거나 경계 뒤 문단 끝 서식을 첫 줄에 합산.
#[test]
fn first_line_stops_at_existing_forced_boundaries() {
    for br in ["page", "column"] {
        for text in ["한글", ""] {
            let p = height_paragraph("auto", 240, 22, &format!("{text}</w:t><w:br w:type='{br}'/><w:t>"))
                .replace("</w:r></w:p>", "</w:r><w:r><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='80'/></w:rPr><w:t>큰</w:t></w:r></w:p>")
                .replace("<w:pPr>", "<w:pPr><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='80'/></w:rPr>");
            let d = read("", &small_table("", &height_cell("", &p)));
            assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 1902, "{br}/{text}");
        }
    }
}
// 이것을 실패시키는 것: 첫 문단만 측정, 문단 간격 합산/누락, contextual 결합 결과 무시, 셀 외곽 간격 누락.
#[test]
fn every_source_paragraph_supplies_one_line_and_collapsed_gap() {
    for (before, contextual, expected) in [(80, false, 8404), (200, false, 8804), (200, true, 7804)]
    {
        let context = if contextual { "<w:contextualSpacing/>" } else { "" };
        let a = height_paragraph("auto", 240, 22, "한글")
            .replace("<w:pPr>", &format!("<w:pPr>{context}"))
            .replace("w:line='240'", "w:line='240' w:before='400' w:after='120'");
        let b = height_paragraph("auto", 240, 22, "한글")
            .replace("<w:pPr>", &format!("<w:pPr>{context}"))
            .replace("w:line='240'", &format!("w:line='240' w:before='{before}' w:after='400'"));
        let d = read("", &small_table("", &height_cell("", &format!("{a}{b}"))));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected);
    }
    let p = height_paragraph("exact", 20000000, 22, "한글");
    assert!(
        decode_docx(&package(&small_table("", &height_cell("", &format!("{p}{p}"))), &[])).is_err()
    );
}

// 이것을 실패시키는 것: 같은 시작 행의 짧은 병합에 행 최소값만 넣거나 일반 셀 높이 보존 누락.
#[test]
fn explicit_starting_row_heights_include_shorter_merge_minima() {
    let paragraph = height_paragraph("exact", 0, 22, "");
    let ordinary = height_cell("", &paragraph);
    let restart = height_cell("<w:vMerge w:val='restart'/>", &paragraph);
    let continuation = height_cell("<w:vMerge/>", &paragraph);
    let body = format!(
        "<w:tbl><w:tr><w:trPr><w:trHeight w:val='200'/></w:trPr>{restart}{restart}{ordinary}</w:tr><w:tr><w:trPr><w:trHeight w:val='600'/></w:trPr>{continuation}{continuation}{ordinary}</w:tr><w:tr><w:trPr><w:trHeight w:val='1000'/></w:trPr>{continuation}{restart}{restart}</w:tr><w:tr><w:trPr><w:trHeight w:val='1400'/></w:trPr>{continuation}{continuation}{continuation}</w:tr></w:tbl>"
    );
    let d = read("", &body);
    let ts = tables(&d);
    let t = &ts[0];
    assert_eq!(t.rows[0].cells[0].row_span, 4);
    assert_eq!(t.rows[0].cells[0].height.unwrap().as_i32(), 16000);
    assert_eq!(t.rows[0].cells[1].row_span, 2);
    assert_eq!(t.rows[0].cells[1].height.unwrap().as_i32(), 4000);
    assert_eq!(t.rows[0].cells[2].height.unwrap().as_i32(), 1000);
    assert!(t.rows[1].cells[0].height.is_none());
}

// 이것을 실패시키는 것: 병합 없는 행에도 셀 높이를 명시하여 row.height fallback 경로 변경.
#[test]
fn unmerged_rows_retain_height_fallback() {
    let c = height_cell("", &height_paragraph("exact", 0, 22, ""));
    let body = format!(
        "<w:tbl><w:tr><w:trPr><w:trHeight w:val='200'/></w:trPr>{c}{c}</w:tr><w:tr><w:trPr><w:trHeight w:val='600'/></w:trPr>{c}{c}</w:tr><w:tr><w:trPr><w:trHeight w:val='1000'/></w:trPr>{c}{c}</w:tr></w:tbl>"
    );
    let d = read("", &body);
    let ts = tables(&d);
    for (row, height) in ts[0].rows.iter().zip([1000, 3000, 5000]) {
        assert_eq!(row.height.unwrap().as_i32(), height);
        assert!(row.cells.iter().all(|cell| cell.height.is_none()));
    }
}

// 이것을 실패시키는 것: 병합 구간 합 범위 검증 누락 또는 단독 restart를 실제 rowSpan으로 취급.
#[test]
fn merged_minimum_range_and_unextended_restart() {
    let c = height_cell("<w:vMerge w:val='restart'/>", &height_paragraph("auto", 240, 22, "한글"));
    let d = read("", &format!("<w:tbl><w:tr>{c}</w:tr></w:tbl>"));
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 1902);
    let row_pr = "<w:trPr><w:trHeight w:val='20000000'/></w:trPr>";
    let body = format!(
        "<w:tbl><w:tr>{row_pr}{c}</w:tr><w:tr>{row_pr}{}</w:tr></w:tbl>",
        cell("<w:vMerge/>", "")
    );
    let error = decode_docx(&package(&body, &[])).unwrap_err().to_string();
    assert!(error.contains("merged minimum height overflow"), "{error}");
}

// 이것을 실패시키는 것: 강제 페이지 시작에서 앞 문단 after까지 없애거나 제거된 before를 셀 하한에 더함.
#[test]
fn forced_paragraph_start_retains_only_previous_after() {
    for start in ["<w:pageBreakBefore/>", ""] {
        let first = height_paragraph("auto", 240, 22, "한글")
            .replace("w:line='240'", "w:line='240' w:after='120'");
        let second = height_paragraph("auto", 240, 22, "한글")
            .replace("<w:pPr>", &format!("<w:pPr>{start}"))
            .replace("w:line='240'", "w:line='240' w:before='200'");
        let second = if start.is_empty() {
            second.replace("<w:t>", "<w:br w:type='page'/><w:t>")
        } else {
            second
        };
        let d = read("", &small_table("", &height_cell("", &format!("{first}{second}"))));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 4404);
    }
}

// 이것을 실패시키는 것: 기본 스타일에서 상속한 마지막 원본 문단 뒤 10pt 누락.
#[test]
fn cell_minimum_includes_single_paragraph_after() {
    let styles = "<w:style w:type='paragraph' w:default='1' w:styleId='body'><w:pPr><w:spacing w:before='0' w:after='200' w:line='276' w:lineRule='auto'/></w:pPr></w:style>";
    let p = height_paragraph("auto", 276, 22, "한글");
    let d = read(styles, &small_table("", &height_cell("", &p)));
    let ts = tables(&d);
    // 기존 첫 줄 2187 + 마지막 뒤 1000 (HWPUNIT).
    assert_eq!(ts[0].rows[0].height.unwrap().as_i32(), 3187);
    let p = &ts[0].rows[0].cells[0].paragraphs[0];
    let shape = &d.registry.para_shapes[p.para_shape_id.get()];
    assert_eq!((shape.space_before.as_i32(), shape.space_after.as_i32()), (0, 1000));
}

// 이것을 실패시키는 것: 두 문단의 내부 결합 간격만 더하고 마지막 뒤 6pt 누락.
#[test]
fn cell_minimum_includes_last_of_two_paragraphs_after() {
    let p = height_paragraph("auto", 240, 22, "한글")
        .replace("w:line='240'", "w:line='240' w:after='120'");
    let d = read("", &small_table("", &height_cell("", &format!("{p}{p}"))));
    // 기존 첫 줄 둘과 내부 간격 4404 + 마지막 뒤 600.
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 5004);
}

// 이것을 실패시키는 것: 첫 원본 문단 앞 12pt 누락.
#[test]
fn cell_minimum_includes_only_first_paragraph_before() {
    let p = height_paragraph("auto", 240, 22, "한글")
        .replace("w:line='240'", "w:line='240' w:before='240'");
    let d = read("", &small_table("", &height_cell("", &p)));
    let ts = tables(&d);
    // 기존 첫 줄 1902 + 첫 앞 1200.
    assert_eq!(ts[0].rows[0].height.unwrap().as_i32(), 3102);
    let p = &ts[0].rows[0].cells[0].paragraphs[0];
    let shape = &d.registry.para_shapes[p.para_shape_id.get()];
    assert_eq!((shape.space_before.as_i32(), shape.space_after.as_i32()), (1200, 0));
}

// 이것을 실패시키는 것: 중첩 표로 끝나도 앞 문단 뒤 간격 유지 또는 중첩 표 뒤 원본 문단의 뒤 간격 누락.
#[test]
fn cell_minimum_omits_after_when_nested_table_ends_cell() {
    let p = height_paragraph("auto", 240, 22, "한글")
        .replace("w:line='240'", "w:line='240' w:after='120'");
    let nested = small_table("", &height_cell("", &height_paragraph("auto", 240, 22, "한글")));
    for (content, expected) in [
        (format!("{p}{nested}"), 1902),
        (format!("{nested}{p}"), 2502),
        (format!("{p}{nested}{p}"), 4404),
    ] {
        let d = read("", &small_table("", &height_cell("", &content)));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected, "{content}");
    }
}

// 이것을 실패시키는 것: 자동 간격을 기존 경로와 다르게 재계산하거나 무시된 직접 twip 값을 사용.
#[test]
fn cell_minimum_reuses_effective_autospacing_edges() {
    let styles = "<w:style w:type='paragraph' w:default='1' w:styleId='body'><w:pPr><w:spacing w:before='200' w:after='300'/></w:pPr></w:style>";
    let p = height_paragraph("auto", 240, 22, "한글").replace(
        "w:line='240'",
        "w:line='240' w:before='240' w:after='240' w:beforeAutospacing='1' w:afterAutospacing='1'",
    );
    let d = read(styles, &small_table("", &height_cell("", &p)));
    // 기존 간격 해석은 자동 플래그가 있는 직접 값을 생략하고 상속값을 유지한다.
    assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 4402);
}

// 이것을 실패시키는 것: 첫 강제 줄만 측정하거나 br/textWrapping/cr 중 하나를 누락.
#[test]
fn forced_lines_sum_each_existing_word_height() {
    for br in ["<w:br/>", "<w:br w:type='textWrapping'/>", "<w:cr/>"] {
        for (texts, expected) in [(["가", "나", "다"], 5706), (["A", "B", "C"], 4389)] {
            let text = texts.join(&format!("</w:t>{br}<w:t>"));
            let p = height_paragraph("auto", 240, 22, &text);
            let d = read("", &small_table("", &height_cell("", &p)));
            // Existing metrics: Korean 1902 × 3; Latin 1463 × 3.
            assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected, "{br}/{text}");
        }
    }
}

// 이것을 실패시키는 것: 문단 전체의 최대 크기를 모든 줄에 적용하거나 이전 줄의 높이를 초기화하지 않음.
#[test]
fn forced_lines_use_their_own_run_sizes() {
    for (first, second) in [(22, 32), (32, 22)] {
        let p = height_paragraph("auto", 240, first, "한</w:t><w:br/><w:t>").replace(
            "</w:r></w:p>",
            &format!("</w:r><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='{second}'/></w:rPr><w:t>글</w:t></w:r></w:p>"),
        );
        let d = read("", &small_table("", &height_cell("", &p)));
        // 11pt = 1902, 16pt = 2767; round each line before summing.
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 4669);
    }
}

// 이것을 실패시키는 것: 끝 빈 줄 생략 또는 빈 줄 위치의 run/문단 끝 서식 대신 앞 줄 서식 사용.
#[test]
fn forced_lines_include_trailing_empty_line_format() {
    for (p, expected) in [
        (height_paragraph("auto", 240, 22, "A</w:t><w:br/><w:t>"), 3365),
        (height_paragraph("auto", 240, 22, "한</w:t><w:br/><w:t>"), 3804),
        (height_paragraph("auto", 240, 22, "한</w:t><w:br/><w:t>").replace(
            "<w:pPr>",
            "<w:pPr><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='40'/></w:rPr>",
        ), 5360),
        (height_paragraph("auto", 240, 22, "한</w:t><w:br/><w:t>").replace(
            "</w:r></w:p>",
            "</w:r><w:r><w:rPr><w:rFonts w:eastAsia='맑은 고딕'/><w:sz w:val='32'/></w:rPr></w:r></w:p>",
        ), 4669),
    ] {
        let d = read("", &small_table("", &height_cell("", &p)));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected, "{p}");
    }
}

// 이것을 실패시키는 것: page/column 뒤 조각까지 합산하거나 그 뒤 문단 끝 서식을 앞 조각에 적용.
#[test]
fn forced_lines_stop_at_page_and_column_first_fragment() {
    for br in ["page", "column"] {
        for (text, expected) in [("A", 1463), ("한", 1902)] {
            let p = height_paragraph(
                "auto",
                240,
                22,
                &format!("{text}</w:t><w:br w:type='{br}'/><w:t>B"),
            );
            for p in [p.clone(), p.replace(
                "<w:t>B</w:t>",
                "</w:r><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='32'/></w:rPr><w:t>B</w:t>",
            )] {
                let d = read("", &small_table("", &height_cell("", &p)));
                assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), expected, "{br}/{text}");
            }
        }
        let p = height_paragraph(
            "exact",
            200,
            22,
            &format!("A</w:t><w:br/><w:t>B</w:t><w:br w:type='{br}'/><w:t>C"),
        );
        let d = read("", &small_table("", &height_cell("", &p)));
        assert_eq!(tables(&d)[0].rows[0].height.unwrap().as_i32(), 2000);
    }
}
