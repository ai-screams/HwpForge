use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::table::TableVerticalAlign;
use hwpforge_core::{LayoutProfile, RunContent, Table};
use hwpforge_foundation::LineSpacingType;

fn read(styles: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
fn p(properties: &str, text: &str) -> String {
    format!("<w:p><w:pPr>{properties}</w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
}
fn table(text: &str) -> String {
    format!(
        "<w:tbl><w:tblGrid><w:gridCol w:w='1000'/></w:tblGrid><w:tr><w:tc>{}</w:tc></w:tr></w:tbl>",
        p("", text)
    )
}
fn table_run(run: &hwpforge_core::Run) -> &Table {
    match &run.content {
        RunContent::Table(t) => t,
        other => panic!("{other:?}"),
    }
}
const DEFAULTS: &str = "<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr></w:rPrDefault></w:docDefaults>";

#[test]
fn imported_profile_and_auto_multipliers_agree_with_named_styles() {
    // 이것을 실패시키는 것: 프로필 설정 삭제, auto 자연 높이 환산을 정의 또는 본문에 유지.
    for (line, expected) in [(240, 100.0), (276, 115.0), (360, 150.0)] {
        let styles = format!("{DEFAULTS}<w:style w:type='paragraph' w:styleId='named'><w:pPr><w:spacing w:line='{line}' w:lineRule='auto'/></w:pPr></w:style>");
        let d = read(&styles, &p("<w:pStyle w:val='named'/>", "한글"));
        assert_eq!(d.document.layout_profile(), LayoutProfile::WordCompatible);
        let para = &d.document.sections()[0].paragraphs[0];
        let named = d.registry.style_entries().values().next().unwrap();
        for index in [para.para_shape_id, named.para_shape_id] {
            let shape = &d.registry.para_shapes[index.get()];
            assert_eq!(
                (shape.line_spacing_type, shape.line_spacing_value),
                (LineSpacingType::Percentage, expected)
            );
        }
    }
    for (rule, line, kind, expected) in [
        ("exact", 400, LineSpacingType::Fixed, 4000.0),
        ("atLeast", 360, LineSpacingType::AtLeast, 3804.0),
    ] {
        let d = read(
            DEFAULTS,
            &p(&format!("<w:spacing w:line='{line}' w:lineRule='{rule}'/>"), "한글"),
        );
        let shape =
            &d.registry.para_shapes[d.document.sections()[0].paragraphs[0].para_shape_id.get()];
        assert_eq!((shape.line_spacing_type, shape.line_spacing_value), (kind, expected));
    }
}

#[test]
fn unspecified_and_filler_cells_are_top_aligned() {
    // 이것을 실패시키는 것: 원본/합성 셀의 미지정 정렬을 None으로 남김, 명시 정렬 덮어쓰기.
    let mut cells = String::new();
    for valign in ["", "top", "center", "bottom", "unknown"] {
        let props =
            if valign.is_empty() { String::new() } else { format!("<w:vAlign w:val='{valign}'/>") };
        cells.push_str(&format!("<w:tc><w:tcPr>{props}</w:tcPr><w:p/></w:tc>"));
    }
    let d = read(
        "",
        &format!("<w:tbl><w:tr><w:trPr><w:gridBefore w:val='1'/></w:trPr>{cells}</w:tr></w:tbl>"),
    );
    let t = table_run(&d.document.sections()[0].paragraphs[0].runs[0]);
    assert_eq!(
        t.rows[0].cells.iter().map(|c| c.vertical_align).collect::<Vec<_>>(),
        [
            Some(TableVerticalAlign::Top),
            Some(TableVerticalAlign::Top),
            Some(TableVerticalAlign::Top),
            Some(TableVerticalAlign::Center),
            Some(TableVerticalAlign::Bottom),
            Some(TableVerticalAlign::Top)
        ]
    );
}

#[test]
fn consecutive_tables_anchor_in_source_order_without_crossing_spacing() {
    // 이것을 실패시키는 것: 합성 holder 미제거, 역순 삽입, 앞뒤 간격의 표 너머 결합.
    let body = format!(
        "{}{}{}{}",
        p("<w:spacing w:after='480'/>", "before"),
        table("one"),
        table("two"),
        p("<w:spacing w:before='240' w:after='240'/>", "after")
    );
    let d = read(DEFAULTS, &body);
    let ps = &d.document.sections()[0].paragraphs;
    assert_eq!(ps.len(), 2);
    assert_eq!(table_run(&ps[1].runs[0]).rows[0].cells[0].paragraphs[0].text_content(), "one");
    assert_eq!(table_run(&ps[1].runs[1]).rows[0].cells[0].paragraphs[0].text_content(), "two");
    assert_eq!(ps[1].runs[2].content.as_text(), Some("after"));
    assert_eq!(d.registry.para_shapes[ps[0].para_shape_id.get()].space_after.as_i32(), 2400);
    let shape = &d.registry.para_shapes[ps[1].para_shape_id.get()];
    assert_eq!((shape.space_before.as_i32(), shape.space_after.as_i32()), (1200, 1200));
}

#[test]
fn explicit_empty_paragraph_is_an_anchor_but_boundaries_keep_holders() {
    // 이것을 실패시키는 것: 빈 문단 내용 추론 삭제, 명시 쪽 경계를 넘는 이동, 끝 anchor text 누락.
    let d = read("", &format!("{}<w:p/>{}", table("one"), p("", "after")));
    let ps = &d.document.sections()[0].paragraphs;
    assert_eq!(ps.len(), 2);
    assert_eq!(ps[0].runs.len(), 2);
    assert_eq!(ps[0].runs[1].content.as_text(), Some(""));
    assert_eq!(ps[1].text_content(), "after");
    for next in [
        String::new(),
        p("<w:pageBreakBefore/>", "after"),
        "<w:p><w:r><w:br w:type='page'/><w:t>after</w:t></w:r></w:p>".into(),
    ] {
        let d = read("", &format!("{}{next}", table("one")));
        let holder = &d.document.sections()[0].paragraphs[0];
        assert_eq!(holder.runs.len(), 2);
        assert_eq!(holder.runs[1].content.as_text(), Some(""));
        assert_eq!(table_run(&holder.runs[0]).rows[0].cells[0].paragraphs[0].text_content(), "one");
    }
}

#[test]
fn column_break_keeps_the_table_before_the_break() {
    // 이것을 실패시키는 것: column break 경계 누락으로 앞 표를 뒤 조각에 연결.
    let d = read(
        "",
        &format!("{}<w:p><w:r><w:br w:type='column'/><w:t>after</w:t></w:r></w:p>", table("one")),
    );
    let ps = &d.document.sections()[0].paragraphs;
    assert_eq!(ps.len(), 3);
    assert_eq!(ps[0].runs[1].content.as_text(), Some(""));
    assert_eq!(table_run(&ps[0].runs[0]).rows[0].cells[0].paragraphs[0].text_content(), "one");
    assert_eq!(ps[1].text_content(), "");
    assert_eq!(ps[1].runs.len(), 1);
    assert_eq!(ps[2].text_content(), "after");
    assert_eq!(ps[2].runs.len(), 1);
    assert!(ps[2].page_break);
    assert!(d.warnings.iter().any(|w| w.loss() == "column_break"));
}

#[test]
fn nested_anchors_move_only_after_width_plans_inside_their_cell() {
    // 이것을 실패시키는 것: 폭 확정 전 이동, 중첩 셀 밖으로 이동, 셀 끝 anchor 삭제.
    for following in ["", "<w:p><w:r><w:t>inside</w:t></w:r></w:p>"] {
        let body = format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='2000'/></w:tblPr><w:tr><w:tc><w:tcPr><w:tcMar><w:left w:w='100'/><w:right w:w='100'/></w:tcMar></w:tcPr>{}{following}</w:tc></w:tr></w:tbl>{}", table("nested"), p("", "outside"));
        let d = read("", &body);
        let ps = &d.document.sections()[0].paragraphs;
        assert_eq!(ps.len(), 1);
        let outer = table_run(&ps[0].runs[0]);
        assert_eq!(outer.width.unwrap().as_i32(), 10000);
        let cell = &outer.rows[0].cells[0];
        assert_eq!(cell.paragraphs.len(), 1);
        let nested = table_run(&cell.paragraphs[0].runs[0]);
        assert_eq!(nested.width.unwrap().as_i32(), 9000);
        assert_eq!(
            cell.paragraphs[0].runs[1].content.as_text(),
            Some(if following.is_empty() { "" } else { "inside" })
        );
        assert_eq!(ps[0].runs[1].content.as_text(), Some("outside"));
    }
}

#[test]
fn minimum_spacing_and_cell_height_still_diagnose_metric_approximations() {
    // 이것을 실패시키는 것: auto 경고 정리와 함께 atLeast/셀 메트릭 근사까지 삭제.
    for cell in [false, true] {
        let paragraph = p(if cell { "<w:spacing w:line='240'/>" } else { "<w:spacing w:line='0' w:lineRule='atLeast'/>" }, "A가").replace("<w:r>", "<w:r><w:rPr><w:rFonts w:ascii='Calibri' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr>");
        let body = if cell {
            format!("<w:tbl><w:tr><w:tc>{paragraph}</w:tc></w:tr></w:tbl>")
        } else {
            paragraph
        };
        let d = read("", &body);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "mixed_font_line_height")
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
        if cell {
            assert_eq!(
                table_run(&d.document.sections()[0].paragraphs[0].runs[0]).rows[0]
                    .height
                    .unwrap()
                    .as_i32(),
                1940
            );
        } else {
            let p = &d.document.sections()[0].paragraphs[0];
            assert_eq!(d.registry.para_shapes[p.para_shape_id.get()].line_spacing_value, 3880.0);
        }
    }
}
