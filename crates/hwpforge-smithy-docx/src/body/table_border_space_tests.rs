use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::Table;
fn read(styles: &str, body: &str) -> DocxDecoded {
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap()
}
fn cell(pr: &str, body: &str) -> String {
    format!("<w:tc><w:tcPr>{pr}</w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p>{body}</w:tc>")
}
fn borders(line: &str, sz: i32) -> String {
    format!(
        "<w:tblBorders>{}</w:tblBorders>",
        ["left", "right", "top", "bottom", "insideH", "insideV"]
            .map(|s| format!("<w:{s} w:val='{line}' w:sz='{sz}'/>"))
            .join("")
    )
}
fn table(pr: &str, rows: &str) -> String {
    format!("<w:tbl><w:tblPr>{pr}</w:tblPr>{rows}</w:tbl>")
}
fn one(pr: &str, cp: &str) -> String {
    table(pr, &format!("<w:tr>{}</w:tr>", cell(cp, "")))
}
fn tables(d: &DocxDecoded) -> Vec<Table> {
    let mut ts = vec![];
    d.document.for_each_table(|t| ts.push(t.clone()));
    ts
}
fn widths(d: &DocxDecoded, t: &Table) -> Vec<[Option<i32>; 4]> {
    std::iter::once(t.border_fill_id.unwrap())
        .chain(t.rows.iter().flat_map(|r| r.cells.iter().map(|c| c.border_fill_id.unwrap())))
        .map(|id| {
            let b = &d.registry.border_fill(id).unwrap().border;
            [b.left, b.right, b.top, b.bottom].map(|s| s.width.map(|w| w.as_i32()))
        })
        .collect()
}
fn warnings(d: &DocxDecoded) -> u64 {
    d.warnings
        .iter()
        .filter(|w| w.loss() == "table_border_space" && w.action() == "retain_margins")
        .map(|w| w.count())
        .sum()
}
// 이것을 실패시키는 것: 원본 single/double 수치로 선 종류 추정, 양수 exact 또는 공간 속성 사실 누락.
#[test]
fn source_admission_preserves_widths_only_for_supported_tables() {
    for (pr, cp, admit) in [
        (borders("single", 4), "".to_string(), true),
        (borders("single", 3), "".to_string(), true),
        (format!("{}<w:tblOverlap w:val='never'/>", borders("single", 4)), "".to_string(), true),
        (
            borders("single", 4),
            "<w:tcBorders><w:top w:val='single' w:sz='4' w:space='000'/></w:tcBorders>".into(),
            true,
        ),
        (
            format!("{}<w:tblCellSpacing w:w='000' w:type='dxa'/>", borders("single", 4)),
            "".to_string(),
            true,
        ),
        (borders("double", 4), "".to_string(), false),
        (
            format!("{}<w:tblCellSpacing w:w='20' w:type='dxa'/>", borders("single", 4)),
            "".to_string(),
            false,
        ),
        (
            borders("single", 4),
            "<w:tcBorders><w:top w:val='single' w:sz='4' w:space='1'/></w:tcBorders>".into(),
            false,
        ),
        (
            borders("single", 4),
            "<w:tcBorders><w:top w:val='single' w:sz='4' w:color='FF0000'/></w:tcBorders>".into(),
            true,
        ),
        (borders("single", 0), "".to_string(), false),
    ] {
        let d = read("", &one(&pr, &cp));
        let ts = tables(&d);
        assert_eq!(warnings(&d), u64::from(!admit));
        assert!(widths(&d, &ts[0]).iter().flatten().all(|w| w.is_some() == admit));
    }
    for rule in ["exact", "auto"] {
        for h in [0, 600] {
            let d=read("",&table(&borders("single",4),&format!("<w:tr><w:trPr><w:trHeight w:val='{h}' w:hRule='{rule}'/></w:trPr>{}</w:tr><w:tr>{}</w:tr>",cell("",""),cell("",""))));
            let admit = rule != "exact" || h == 0;
            assert_eq!(warnings(&d), u64::from(!admit));
            assert_eq!(widths(&d, &tables(&d)[0])[0], [admit.then_some(50); 4]);
        }
    }
}
// 이것을 실패시키는 것: overlay 때 불확정 사실 삭제, 선택 사슬과 무관한 스타일 합침, tcPr 소유자 소실.
#[test]
fn selected_style_uncertainty_survives_partial_overlays() {
    for unsupported in [
        "<w:tblStylePr w:type='firstRow'><w:tblPr><w:tblBorders><w:top w:val='double'/></w:tblBorders></w:tblPr></w:tblStylePr>".to_string(),
        "<w:tblStylePr w:type='wholeTable'><w:tblPr><w:tblCellMar><w:top w:w='20'/></w:tblCellMar></w:tblPr></w:tblStylePr>".into(),
        "<w:tcPr><w:tcMar><w:left w:w='20'/></w:tcMar></w:tcPr>".into(),
        format!("<w:tblPr>{}</w:tblPr>",borders("double",4)),
    ] {
        let styles=format!("<w:style w:type='table' w:styleId='base'>{unsupported}</w:style><w:style w:type='table' w:styleId='child'><w:basedOn w:val='base'/><w:tblPr>{}</w:tblPr></w:style>",borders("single",4));
        let d=read(&styles,&format!("{}{}",one(&format!("<w:tblStyle w:val='child'/>{}",borders("single",4)),"<w:tcBorders><w:top w:val='single' w:sz='4'/></w:tcBorders>"),one(&borders("single",4),"")));
        let ts=tables(&d);assert_eq!(widths(&d,&ts[0])[0],[None;4]);assert_eq!(widths(&d,&ts[1])[0],[Some(50);4]);assert_eq!(warnings(&d),1);
    }
}
// 이것을 실패시키는 것: tblPrEx 차단 전에 사실을 기록하지 않음, 자식 표 사실을 바깥 표에 합침.
#[test]
fn row_exceptions_are_local_to_their_own_table() {
    let normal = one(&borders("single", 4), "");
    let exception=table(&borders("single",4),&format!("<w:tr><w:tblPrEx><w:tblBorders><w:left w:val='double'/></w:tblBorders></w:tblPrEx>{}</w:tr>",cell("","")));
    let outer = table(&borders("single", 4), &format!("<w:tr>{}</w:tr>", cell("", &exception)));
    let d = read("", &format!("{normal}{exception}{outer}"));
    let ts = tables(&d);
    assert_eq!(ts.len(), 4);
    for (t, expected) in ts.iter().zip([Some(50), None, Some(50), None]) {
        assert!(widths(&d, t).iter().flatten().all(|v| *v == expected));
    }
    assert_eq!(warnings(&d), 2);
    assert_eq!(
        d.warnings
            .iter()
            .filter(|w| w.loss() == "table_property_exceptions")
            .map(|w| w.count())
            .sum::<u64>(),
        2
    );
}
// 이것을 실패시키는 것: continuation 사실 소실, 병합 측면을 max로 축약, 마지막 bottom=nil 미보존.
#[test]
fn merged_source_faces_and_continuations_keep_space_costs() {
    for cp in [
        "<w:tcBorders><w:left w:val='double' w:sz='4'/></w:tcBorders>",
        "<w:tcBorders><w:left w:val='single' w:sz='12'/></w:tcBorders>",
    ] {
        let d = read(
            "",
            &table(
                &borders("single", 4),
                &format!(
                    "<w:tr>{}</w:tr><w:tr>{}</w:tr>",
                    cell("<w:vMerge w:val='restart'/>", ""),
                    cell(&format!("<w:vMerge/>{cp}"), "")
                ),
            ),
        );
        assert_eq!(warnings(&d), 1);
        assert!(widths(&d, &tables(&d)[0]).iter().flatten().all(Option::is_none));
    }
    let d = read(
        "",
        &table(
            &borders("single", 4),
            &format!(
                "<w:tr>{}</w:tr><w:tr>{}</w:tr>",
                cell("<w:vMerge w:val='restart'/>", ""),
                cell("<w:vMerge/><w:tcBorders><w:bottom w:val='nil'/></w:tcBorders>", "")
            ),
        ),
    );
    assert_eq!(warnings(&d), 0);
    assert_eq!(widths(&d, &tables(&d)[0])[1], [Some(50), Some(50), Some(50), None]);
}

// 이것을 실패시키는 것: 병합 면의 서로 다른 구간을 최댓값으로 축약, orphan continuation을 병합 내부로 처리.
#[test]
fn horizontal_merge_segments_defer_but_orphan_continuations_are_admitted() {
    let d = read(
        "",
        &table(
            &borders("single", 4),
            &format!(
                "<w:tr>{}</w:tr><w:tr>{}{}</w:tr>",
                cell("<w:gridSpan w:val='2'/>", ""),
                cell("", ""),
                cell("<w:tcBorders><w:top w:val='single' w:sz='12'/></w:tcBorders>", "")
            ),
        ),
    );
    assert_eq!(warnings(&d), 1);
    assert!(widths(&d, &tables(&d)[0]).iter().flatten().all(Option::is_none));
    let d = read("", &one(&borders("single", 4), "<w:vMerge/>"));
    assert_eq!(warnings(&d), 0);
    assert!(widths(&d, &tables(&d)[0]).iter().flatten().all(|v| *v == Some(50)));
}

fn assert_admission(d: &DocxDecoded, expected: Option<i32>, reason: Option<&str>) {
    let ts = tables(d);
    assert_eq!(ts.len(), 1);
    assert_eq!(widths(d, &ts[0]), vec![[expected; 4]; 2]);
    let ws: Vec<_> = d.warnings.iter().filter(|w| w.loss() == "table_border_space").collect();
    assert_eq!(ws.len(), usize::from(reason.is_some()));
    assert_eq!(warnings(d), u64::from(reason.is_some()));
    if let Some(reason) = reason {
        assert_eq!(ws[0].message(), format!("Table border space source admission failed ({reason}); existing cell margins are retained"));
        assert_eq!(ws[0].action(), "retain_margins");
    }
}

// 이것을 실패시키는 것: 스타일 행 하위 트리 소실, 상속/default/wholeTable 소유자 소실, 직접 덮어쓰기로 사실 삭제.
#[test]
fn style_row_formatting_is_owned_by_the_selected_style_chain() {
    for whole in [false, true] {
        let row = "<w:trPr><w:trHeight w:val='600' w:hRule='exact'/></w:trPr>";
        let row = if whole {
            format!("<w:tblStylePr w:type='wholeTable'>{row}</w:tblStylePr>")
        } else {
            row.into()
        };
        for selection in ["direct", "basedOn", "default", "unused"] {
            let default = if selection == "default" { "w:default='1'" } else { "" };
            let styles = format!("<w:style w:type='table' w:styleId='s' {default}>{row}<w:tblPr>{}</w:tblPr></w:style><w:style w:type='table' w:styleId='child'><w:basedOn w:val='s'/></w:style>", borders("single", 4));
            let selector = match selection {
                "direct" => "<w:tblStyle w:val='s'/>",
                "basedOn" => "<w:tblStyle w:val='child'/>",
                _ => "",
            };
            for partial in [false, true] {
                let cp = if partial {
                    "<w:tcBorders><w:top w:val='single' w:sz='4'/></w:tcBorders>"
                } else {
                    ""
                };
                let pr = format!(
                    "{selector}{}{}",
                    borders("single", 4),
                    if partial { "<w:tblCellMar><w:left w:w='108'/></w:tblCellMar>" } else { "" }
                );
                let body = one(&pr, cp);
                let body = if partial {
                    body.replace(
                        "<w:tr>",
                        "<w:tr><w:trPr><w:trHeight w:val='200' w:hRule='atLeast'/></w:trPr>",
                    )
                } else {
                    body
                };
                let d = read(&styles, &body);
                let unused = selection == "unused";
                assert_admission(
                    &d,
                    unused.then_some(50),
                    (!unused).then_some("unsupported_source_format"),
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 생략 내부선이 공통 space/미지원 속성 검사를 우회하거나 0까지 보류.
#[test]
fn omitted_cell_internal_borders_share_source_attribute_admission() {
    for side in ["insideH", "insideV"] {
        for line in ["nil", "none"] {
            for (attributes, admit) in [
                ("w:space='1'", false),
                ("w:space='0'", true),
                ("w:space='000'", true),
                ("w:shadow='1'", false),
                ("xmlns:x='urn:extension' x:space='1'", false),
            ] {
                let cp =
                    format!("<w:tcBorders><w:{side} w:val='{line}' {attributes}/></w:tcBorders>");
                let d = read("", &one(&borders("single", 4), &cp));
                assert_admission(
                    &d,
                    admit.then_some(50),
                    (!admit).then_some("unsupported_source_border"),
                );
                assert_eq!(
                    d.warnings
                        .iter()
                        .filter(|w| w.loss() == "cell_internal_border" && w.action() == "omit")
                        .map(|w| w.count())
                        .sum::<u64>(),
                    1
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 미지원 행/격자/속성/비-W 자식을 읽기 전에 버리거나 leaf 자식이 본문으로 탈출.
#[test]
fn discarded_table_formatting_defaults_to_local_uncertainty() {
    let normal = one(&borders("single", 4), "");
    for (old, replacement) in [
        ("<w:tr>", "<w:tr><w:trPr><w:cnfStyle w:val='100000000000'/></w:trPr>"),
        ("<w:tr>", "<w:tr><w:trPr><w:trHeight w:val='600' w:hRule='future'/></w:trPr>"),
        ("<w:tr>", "<w:tr><w:trPr><x:height xmlns:x='urn:extension'/></w:trPr>"),
        ("<w:tr>", "<w:tr><w:unknownFormat/>"),
        ("<w:tr>", "<w:tr x:height='1' xmlns:x='urn:extension'>"),
        ("<w:tbl>", "<w:tbl><w:tblGrid><w:gridCol w:w='2000'/><w:unknownGrid/></w:tblGrid>"),
        ("<w:tbl>", "<w:tbl><x:format xmlns:x='urn:extension'/>"),
        ("<w:tbl>", "<w:tbl x:format='1' xmlns:x='urn:extension'>"),
        ("<w:tblPr>", "<w:tblPr><x:format xmlns:x='urn:extension'/>"),
        ("<w:tblPr>", "<w:tblPr><w:tblW w:w='2000' x:format='1' xmlns:x='urn:extension'/>"),
        ("<w:tcPr>", "<w:tcPr x:format='1' xmlns:x='urn:extension'>"),
        ("<w:tcPr>", "<w:tcPr><x:format xmlns:x='urn:extension'/>"),
        ("<w:tcPr>", "<w:tcPr><w:vAlign w:val='top' x:format='1' xmlns:x='urn:extension'/>"),
        (
            "<w:tcPr>",
            "<w:tcPr><w:gridSpan w:val='1'><x:format xmlns:x='urn:extension'/></w:gridSpan>",
        ),
    ] {
        let body = normal.replacen(old, replacement, 1);
        let d = read("", &body);
        assert_admission(&d, None, Some("unsupported_source_format"));
    }
}

// 이것을 실패시키는 것: 새 스타일 무시 갈래/확장 속성이 소유자의 기본 보류를 우회함.
#[test]
fn discarded_style_children_and_attributes_keep_the_owner() {
    for content in [
        "<w:unknownFormat/>",
        "<x:format xmlns:x='urn:extension'/>",
        "<w:tblStylePr w:type='wholeTable'><x:format xmlns:x='urn:extension'/></w:tblStylePr>",
        "<w:tcPr><x:format xmlns:x='urn:extension'/></w:tcPr>",
        "<w:tblPr><x:format xmlns:x='urn:extension'/></w:tblPr>",
        "<w:tblPr x:format='1' xmlns:x='urn:extension'/>",
        "<w:tcPr x:format='1' xmlns:x='urn:extension'/>",
        "<w:tblStylePr w:type='wholeTable' x:format='1' xmlns:x='urn:extension'/>",
    ] {
        let styles = format!("<w:style w:type='table' w:styleId='s'>{content}</w:style>");
        let d =
            read(&styles, &one(&format!("<w:tblStyle w:val='s'/>{}", borders("single", 4)), ""));
        assert_admission(&d, None, Some("unsupported_source_format"));
    }
    let paint = "<w:style w:type='table' w:styleId='s'><w:tcPr><w:shd w:fill='FF0000'/></w:tcPr><w:qFormat/><w:rsid w:val='01234567'/></w:style>";
    assert_admission(
        &read(paint, &one(&format!("<w:tblStyle w:val='s'/>{}", borders("single", 4)), "")),
        Some(50),
        None,
    );
    let styles = "<w:style w:type='table' w:styleId='s' xmlns:x='urn:extension' x:format='1'/>";
    let d = read(styles, &one(&format!("<w:tblStyle w:val='s'/>{}", borders("single", 4)), ""));
    assert_admission(&d, None, Some("unsupported_source_format"));
}

// 이것을 실패시키는 것: 내용 손실/continuation 중첩 표/편집 메타데이터를 바깥 표 공간 사실로 합침.
#[test]
fn omitted_content_and_paint_do_not_taint_outer_table_formatting() {
    let hidden = "<w:unknownObject><w:tblPrEx/></w:unknownObject>";
    let d = read(
        "<w:style w:type='table' w:styleId='unused'><w:trPr/></w:style>",
        &one(
            &format!("{}<w:shd w:fill='FF0000'/>", borders("single", 4)),
            "<w:shd w:fill='FF0000'/>",
        ),
    );
    assert_admission(&d, Some(50), None);
    let d = read("", &table(&borders("single", 4), &format!("<w:tr>{}</w:tr>", cell("", hidden))));
    assert_admission(&d, Some(50), None);
    let nested = one(&format!("{}<w:unknownFormat/>", borders("single", 4)), "");
    let d = read(
        "",
        &table(
            &borders("single", 4),
            &format!(
                "<w:tr>{}</w:tr><w:tr>{}</w:tr>",
                cell("<w:vMerge w:val='restart'/>", ""),
                cell("<w:vMerge/>", &nested)
            ),
        ),
    );
    assert_admission(&d, Some(50), None);
}

// 이것을 실패시키는 것: 선택하지 않은 호환성 서식은 소유 표에서 잃거나 내용 분기를 바깥 표와 합침.
#[test]
fn discarded_compatibility_branches_respect_table_formatting_boundaries() {
    let namespace = "xmlns:mc='http://schemas.openxmlformats.org/markup-compatibility/2006'";
    for branch in [
        format!("<mc:AlternateContent {namespace}><mc:Choice Requires='x'><w:tblPrEx/></mc:Choice><mc:Fallback/></mc:AlternateContent>"),
        format!("<mc:unknown {namespace}><w:tblPrEx/></mc:unknown>"),
    ] {
        for boundary in ["<w:tbl>", "<w:tr>"] {
            let body = one(&borders("single", 4), "").replacen(boundary, &format!("{boundary}{branch}"), 1);
            assert_admission(&read("", &body), None, Some("unsupported_source_format"));
        }
        let body = table(&borders("single", 4), &format!("<w:tr>{}</w:tr>", cell("", &branch)));
        assert_admission(&read("", &body), Some(50), None);
    }
}

// 이것을 실패시키는 것: 행 식별자 각각을 공간 서식으로 취급하거나 XML prefix로 namespace를 판정.
#[test]
fn row_identity_attributes_preserve_admitted_widths() {
    for attrs in [
        "id:paraId='12345678'",
        "id:textId='87654321'",
        "id:paraId='12345678' id:textId='87654321'",
    ] {
        let body = one(&borders("single", 4), "").replacen(
            "<w:tr>",
            &format!(
                "<w:tr xmlns:id='http://schemas.microsoft.com/office/word/2010/wordml' {attrs}>"
            ),
            1,
        );
        let d = read("", &body);
        assert_admission(&d, Some(50), None);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "table_attribute" && w.action() == "omit")
                .map(|w| w.count())
                .sum::<u64>(),
            if attrs.contains(' ') { 2 } else { 1 }
        );
    }
}

// 이것을 실패시키는 것: 같은 namespace의 미지 속성, 다른 namespace의 동명 속성, 다른 요소까지 허용.
#[test]
fn row_identity_allowlist_keeps_unknown_attributes_deferred() {
    for (element, uri, attr) in [
        ("tr", "http://schemas.microsoft.com/office/word/2010/wordml", "foo"),
        ("tr", "urn:extension", "paraId"),
        ("tr", "urn:extension", "textId"),
        ("tbl", "http://schemas.microsoft.com/office/word/2010/wordml", "paraId"),
        ("tc", "http://schemas.microsoft.com/office/word/2010/wordml", "textId"),
    ] {
        let body = one(&borders("single", 4), "").replacen(
            &format!("<w:{element}>"),
            &format!("<w:{element} xmlns:w14='{uri}' w14:{attr}='12345678'>"),
            1,
        );
        assert_admission(&read("", &body), None, Some("unsupported_source_format"));
    }
}

// 이것을 실패시키는 것: 식별자 수용이 행 맞춤 또는 양수 exact 높이의 보류 사실을 지움.
#[test]
fn row_identity_does_not_admit_row_layout() {
    for (property, reason) in [
        ("<w:jc w:val='right'/>", "unsupported_source_format"),
        ("<w:trHeight w:val='600' w:hRule='exact'/>", "exact_row"),
    ] {
        let body = one(&borders("single", 4), "").replacen(
            "<w:tr>",
            &format!("<w:tr xmlns:w14='http://schemas.microsoft.com/office/word/2010/wordml' w14:paraId='12345678' w14:textId='87654321'><w:trPr>{property}</w:trPr>"),
            1,
        );
        assert_admission(&read("", &body), None, Some(reason));
    }
}

// 이것을 실패시키는 것: 알려진 쪽 나눔/메타데이터를 기본 보류하거나 미해석 기하를 수용함.
// 각 행의 리터럴 폭과 DOCX 보류 횟수는 분류표나 생산 계산식에서 가져오지 않는다.
#[test]
fn table_vocabulary_has_stable_source_admission() {
    for (parent, property, expected, count) in [
        ("trPr", "<w:tblHeader/>", Some(50), 0),
        ("trPr", "<w:cantSplit/>", Some(50), 0),
        ("trPr", "<w:tblHeader w:val='1'/><w:cantSplit w:val='0'/>", Some(50), 0),
        ("tblPr", "<w:tblLook w:val='04A0' w:firstRow='1' w:lastRow='0' w:firstColumn='1' w:lastColumn='0' w:noHBand='0' w:noVBand='1'/>", Some(50), 0),
        ("tblPr", "<w:tblLayout w:type='fixed'/>", Some(50), 0),
        ("tblPr", "<w:tblW w:w='2000' w:type='dxa'/>", Some(50), 0),
        ("tcPr", "<w:tcW w:w='2000' w:type='dxa'/>", Some(50), 0),
        ("tcPr", "<w:shd w:val='clear' w:color='auto' w:fill='FF0000' w:themeColor='accent1' w:themeTint='80' w:themeShade='80' w:themeFill='accent2' w:themeFillTint='80' w:themeFillShade='80'/>", Some(50), 0),
        ("tcPr", "<w:tcBorders><w:top w:val='single' w:sz='4' w:themeTint='80' w:themeShade='80'/></w:tcBorders>", Some(50), 0),
        ("tcPr", "<w:vAlign w:val='center'/>", Some(50), 0),
        ("tcPr", "<w:gridSpan w:val='1'/>", Some(50), 0),
        ("tcPr", "<w:vMerge w:val='restart'/>", Some(50), 0),
        ("tblPr", "<w:tblCaption w:val='title'/>", Some(50), 0),
        ("tblPr", "<w:tblDescription w:val='description'/>", Some(50), 0),
        ("tblPr", "<w:tblStyleRowBandSize w:val='2'/>", Some(50), 0),
        ("tblPr", "<w:tblStyleColBandSize w:val='2'/>", Some(50), 0),
        ("tblPr", "<w:tblOverlap w:val='never'/>", Some(50), 0),
        ("tblPr", "<w:tblCellSpacing w:w='0' w:type='dxa'/>", Some(50), 0),
        ("tblPr", "<w:tblCellSpacing w:w='20' w:type='dxa'/>", None, 1),
        ("trPr", "<w:cnfStyle w:val='100000000000'/>", None, 1),
        ("tcPr", "<w:cnfStyle w:val='100000000000'/>", None, 1),
        ("trPr", "<w:jc w:val='right'/>", None, 1),
        ("tr", "<w:tblPrEx/>", None, 1),
        ("tblPr", "<w:unknownProperty/>", None, 1),
        ("trPr", "<x:tblHeader xmlns:x='urn:extension'/>", None, 1),
        ("tcPr", "<w:noWrap/>", None, 1),
        ("tcPr", "<w:hideMark/>", None, 1),
        ("tcPr", "<w:textDirection w:val='tbRl'/>", None, 1),
        ("tcPr", "<w:tcFitText/>", None, 1),
        ("tcPr", "<w:hMerge w:val='restart'/>", None, 1),
        ("tcPr", "<w:headers><w:header w:val='a'/></w:headers>", Some(50), 0),
        ("trPr", "<w:hidden/>", None, 1),
        ("trPr", "<w:divId w:val='1'/>", None, 1),
        ("trPr", "<w:wBefore w:w='20' w:type='dxa'/>", None, 1),
        ("trPr", "<w:wAfter w:w='20' w:type='dxa'/>", None, 1),
    ] {
        let body = table(&borders("single", 4), &format!("<w:tr><w:trPr/>{}</w:tr>", cell("", "")))
            .replace("<w:trPr/>", "<w:trPr></w:trPr>")
            .replacen(&format!("<w:{parent}>"), &format!("<w:{parent}>{property}"), 1);
        let decoded = read("", &body);
        assert_eq!(widths(&decoded, &tables(&decoded)[0]), vec![[expected; 4]; 2], "{parent}: {property}");
        assert_eq!(warnings(&decoded), count, "{parent}: {property}");
    }
    // 이것을 실패시키는 것: rsid 이름을 누락하거나 접두사만 같으면 모두 수용함.
    for attribute in ["rsidR", "rsidRPr", "rsidRDefault", "rsidTr", "rsidDel"] {
        let body = one(&borders("single", 4), "")
            .replace("<w:tr>", &format!("<w:tr w:{attribute}='12345678'>"));
        let decoded = read("", &body);
        assert_eq!(widths(&decoded, &tables(&decoded)[0]), vec![[Some(50); 4]; 2]);
        assert_eq!(warnings(&decoded), 0);
    }
    // 이것을 실패시키는 것: 무관 속성이 있는 요소의 미지/타 namespace 속성까지 수용함.
    for (parent, property) in [
        ("tblPr", "<w:tblLook w:future='1'/>"),
        ("tblPr", "<w:tblLayout w:type='fixed' x:type='fixed' xmlns:x='urn:extension'/>"),
        ("tcPr", "<w:shd w:future='1'/>"),
        ("trPr", "<w:tblHeader w:future='1'/>"),
        ("trPr", "<w:cantSplit><w:tblHeader/></w:cantSplit>"),
    ] {
        let body = table(
            &borders("single", 4),
            &format!("<w:tr><w:trPr></w:trPr>{}</w:tr>", cell("", "")),
        )
        .replacen(&format!("<w:{parent}>"), &format!("<w:{parent}>{property}"), 1);
        assert_admission(&read("", &body), None, Some("unsupported_source_format"));
    }
    let body = one(&borders("single", 4), "").replace("<w:tr>", "<w:tr w:rsidFuture='1'>");
    assert_admission(&read("", &body), None, Some("unsupported_source_format"));
}

// 이것을 실패시키는 것: 스타일 행 쪽 나눔을 보류하거나 무관 요소의 자식/속성에서 소유자를 잃음.
#[test]
fn neutral_style_properties_keep_owner_and_conditional_guards() {
    for property in [
        "<w:trPr><w:tblHeader w:val='1'/><w:cantSplit/></w:trPr>",
        "<w:tblPr><w:tblStyleRowBandSize w:val='2'/><w:tblStyleColBandSize w:val='2'/><w:tblLook w:val='04A0'/><w:tblCaption w:val='title'/><w:tblDescription w:val='description'/><w:tblLayout w:type='fixed'/><w:tblOverlap w:val='never'/></w:tblPr>",
        "<w:tcPr><w:shd w:fill='FF0000'/></w:tcPr>",
    ] {
        let styles = format!("<w:style w:type='table' w:styleId='s'>{property}</w:style>");
        assert_admission(&read(&styles, &one(&format!("{}<w:tblStyle w:val='s'/>", borders("single", 4)), "")), Some(50), None);
    }
    for property in [
        "<w:trPr><w:tblHeader w:future='1'/></w:trPr>",
        "<w:trPr><w:cantSplit><w:tblHeader/></w:cantSplit></w:trPr>",
        "<w:tcPr><w:shd w:future='1'/></w:tcPr>",
        "<w:tcPr><w:shd><w:tblHeader/></w:shd></w:tcPr>",
        "<w:tblPr><w:tblStyle w:val='other'/></w:tblPr>",
        "<w:tblPr><w:tblLook w:val='0000'/></w:tblPr><w:tblStylePr w:type='firstRow'><w:trPr><w:tblHeader/></w:trPr></w:tblStylePr>",
    ] {
        let styles = format!("<w:style w:type='table' w:styleId='s'>{property}</w:style>");
        assert_admission(&read(&styles, &one(&format!("{}<w:tblStyle w:val='s'/>", borders("single", 4)), "")), None, Some("unsupported_source_format"));
    }
    let styles = "<w:style w:type='table' w:styleId='s'><w:tblStylePr w:type='wholeTable'><w:trPr><w:tblHeader/><w:cantSplit/></w:trPr></w:tblStylePr></w:style>";
    assert_admission(
        &read(styles, &one(&format!("{}<w:tblStyle w:val='s'/>", borders("single", 4)), "")),
        Some(50),
        None,
    );
}

// 이것을 실패시키는 것: 쪽 나눔 플래그가 Core 표나 폭을 바꾸거나 새 보류 진단을 만듦.
#[test]
fn pagination_flags_preserve_table_output_and_existing_diagnostics() {
    let plain = read("", &one(&borders("single", 4), ""));
    for flags in ["<w:tblHeader/>", "<w:cantSplit/>", "<w:tblHeader/><w:cantSplit/>"] {
        let body = one(&borders("single", 4), "")
            .replace("<w:tr>", &format!("<w:tr><w:trPr>{flags}</w:trPr>"));
        let decoded = read("", &body);
        assert_eq!(tables(&decoded), tables(&plain));
        assert_eq!(widths(&decoded, &tables(&decoded)[0]), vec![[Some(50); 4]; 2]);
        assert_eq!(warnings(&decoded), 0);
        // The pre-existing omission diagnostic is retained; pagination adds no space diagnostic.
        assert_eq!(
            decoded
                .warnings
                .iter()
                .filter(|w| w.loss() == "table_property")
                .map(|w| w.count())
                .sum::<u64>(),
            if flags.contains("/><") { 2 } else { 1 }
        );
    }
}

// 이것을 실패시키는 것: 참/미지 방향의 값 판독 누락, 거짓까지 보류, 상속 또는 직접 덮어쓰기로 사실 삭제.
#[test]
fn direction_values_follow_selected_sources_and_survive_overlays() {
    let margins = "<w:tblCellMar><w:left w:w='108'/><w:right w:w='108'/><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar>";
    for (value, admit) in [
        ("", false),
        ("1", false),
        ("true", false),
        ("on", false),
        ("future", false),
        ("0", true),
        ("false", true),
        ("off", true),
    ] {
        let direction = if value.is_empty() {
            "<w:bidiVisual/>".into()
        } else {
            format!("<w:bidiVisual w:val='{value}'/>")
        };
        for selection in ["direct", "style", "basedOn", "default", "unused"] {
            for overlay in ["", "<w:bidiVisual w:val='false'/>", "<w:bidiVisual/>"] {
                let default = if selection == "default" { "w:default='1'" } else { "" };
                let style_direction = if selection == "direct" { "" } else { &direction };
                let styles = format!("<w:style w:type='table' w:styleId='base' {default}><w:tblPr>{style_direction}</w:tblPr></w:style><w:style w:type='table' w:styleId='child'><w:basedOn w:val='base'/></w:style>");
                let selector = match selection {
                    "direct" => direction.as_str(),
                    "style" => "<w:tblStyle w:val='base'/>",
                    "basedOn" => "<w:tblStyle w:val='child'/>",
                    _ => "",
                };
                let d = read(
                    &styles,
                    &one(
                        &format!("{}{margins}{selector}{overlay}", borders("single", 4)),
                        "<w:tcBorders><w:top w:val='single' w:sz='4'/></w:tcBorders>",
                    ),
                );
                let accepted = (admit || selection == "unused") && overlay != "<w:bidiVisual/>";
                assert_admission(
                    &d,
                    accepted.then_some(50),
                    (!accepted).then_some("unsupported_source_format"),
                );
                let margin = tables(&d)[0].in_margin.unwrap();
                assert_eq!(
                    [
                        margin.left.as_i32(),
                        margin.right.as_i32(),
                        margin.top.as_i32(),
                        margin.bottom.as_i32()
                    ],
                    [540, 540, 0, 0]
                );
            }
        }
    }
}

// 이것을 실패시키는 것: 연결 컨테이너/자식/값/대상 식별자를 보류하거나 기존 생략 진단 제거.
#[test]
fn header_associations_keep_widths_and_omission_diagnostics() {
    for (cp, id, properties, attributes) in [
        ("<w:headers/>", "", 1, 0),
        ("<w:headers><w:header/></w:headers>", "", 2, 0),
        ("<w:headers><w:header w:val='h'/></w:headers>", "", 2, 1),
        ("", " w:id='h'", 0, 1),
        ("<w:headers><w:header w:val='h'/></w:headers>", " w:id='h'", 2, 2),
    ] {
        let body = one(&borders("single", 4), cp).replace("<w:tc>", &format!("<w:tc{id}>"));
        let d = read("", &body);
        assert_admission(&d, Some(50), None);
        for (loss, count) in [("table_property", properties), ("table_attribute", attributes)] {
            assert_eq!(
                d.warnings
                    .iter()
                    .filter(|w| w.loss() == loss && w.action() == "omit")
                    .map(|w| w.count())
                    .sum::<u64>(),
                count
            );
        }
    }
    for cp in [
        "<w:headers w:future='1'/>",
        "<w:headers><w:future/></w:headers>",
        "<w:headers><w:header w:future='1'/></w:headers>",
        "<w:headers><w:header><w:header/></w:header></w:headers>",
    ] {
        assert_admission(
            &read("", &one(&borders("single", 4), cp)),
            None,
            Some("unsupported_source_format"),
        );
    }
    for whole in [false, true] {
        let content = "<w:tcPr><w:headers><w:header w:val='h'/></w:headers></w:tcPr>";
        let content = if whole {
            format!("<w:tblStylePr w:type='wholeTable'>{content}</w:tblStylePr>")
        } else {
            content.into()
        };
        let styles = format!("<w:style w:type='table' w:styleId='s'>{content}</w:style>");
        let d =
            read(&styles, &one(&format!("{}<w:tblStyle w:val='s'/>", borders("single", 4)), ""));
        assert_admission(&d, Some(50), None);
        assert_eq!(
            d.warnings
                .iter()
                .filter(|w| w.loss() == "style_definition")
                .map(|w| w.count())
                .sum::<u64>(),
            1
        );
    }
}

// 이것을 실패시키는 것: 미지원 값이 기본 배치/상속값으로 바뀌면서 원본 불확정을 잃음.
#[test]
fn unsupported_table_values_defer_before_fallback() {
    for property in [
        "<w:jc w:val='both'/>",
        "<w:jc/>",
        "<w:jc w:val='future'/>",
        "<w:tblInd w:w='20' w:type='nil'/>",
        "<w:tblInd w:w='20' w:type='future'/>",
        "<w:tblW w:w='2000' w:type='future'/>",
        "<w:tblLayout w:type='future'/>",
        "<w:tblCellSpacing w:w='0' w:type='future'/>",
    ] {
        for style in [false, true] {
            let styles = if style {
                format!(
                    "<w:style w:type='table' w:styleId='s'><w:tblPr>{property}</w:tblPr></w:style>"
                )
            } else {
                String::new()
            };
            let pr = if style { "<w:tblStyle w:val='s'/>" } else { property };
            assert_admission(
                &read(&styles, &one(&format!("{}{pr}", borders("single", 4)), "")),
                None,
                Some("unsupported_source_format"),
            );
        }
    }
    for cp in ["<w:tcW w:w='2000' w:type='future'/>", "<w:vAlign w:val='future'/>", "<w:vAlign/>"] {
        assert_admission(
            &read("", &one(&borders("single", 4), cp)),
            None,
            Some("unsupported_source_format"),
        );
    }
}

// 이것을 실패시키는 것: 알려진 선호 폭·무시되는 들여쓰기·배경·내용 맞춤을 공간 비용으로 오인.
#[test]
fn known_approximations_preserve_source_space_budget() {
    let base = read("", &one(&borders("single", 4), ""));
    let baseline = tables(&base).remove(0);
    for (parent, property) in [
        ("tblPr", "<w:tblW w:w='2000' w:type='pct'/>"),
        ("tblPr", "<w:tblW w:w='0' w:type='auto'/>"),
        ("tblPr", "<w:tblW w:w='0' w:type='nil'/>"),
        ("tcPr", "<w:tcW w:w='2000' w:type='pct'/>"),
        ("tcPr", "<w:tcW w:w='0' w:type='auto'/>"),
        ("tcPr", "<w:tcW w:w='0' w:type='nil'/>"),
        ("tblPr", "<w:tblInd w:w='20' w:type='pct'/>"),
        ("tblPr", "<w:tblInd w:w='20' w:type='auto'/>"),
        ("tblPr", "<w:tblLayout w:type='autofit'/>"),
        ("tblPr", "<w:tblLayout w:type='fixed'/>"),
        ("tcPr", "<w:vAlign w:val='both'/>"),
        ("tcPr", "<w:vAlign w:val='bottom'/>"),
        ("tblPr", "<w:shd w:val='future' w:fill='FF0000'/>"),
        ("tcPr", "<w:shd w:val='pct20' w:fill='FF0000'/>"),
        ("tblPr", "<w:jc w:val='start'/>"),
        ("tblPr", "<w:jc w:val='end'/>"),
        ("tblPr", "<w:tblInd w:w='20' w:type='dxa'/>"),
    ] {
        let body = one(&borders("single", 4), "").replacen(
            &format!("<w:{parent}>"),
            &format!("<w:{parent}>{property}"),
            1,
        );
        let d = read("", &body);
        assert_admission(&d, Some(50), None);
        let current = tables(&d).remove(0);
        assert_eq!(current.in_margin, baseline.in_margin);
        assert_eq!(current.rows[0].cells[0].height, baseline.rows[0].cells[0].height);
    }
}

// 이것을 실패시키는 것: 누락/다른 종류/순환 스타일 연결이 기본 스타일로 대체되면서 수용됨, 부모 ID 가 빠진 basedOn 을 기록 없이 무시함.
#[test]
fn unresolved_table_style_links_defer_selected_tables_only() {
    for (styles, selector) in [
        ("", "<w:tblStyle w:val='missing'/>") ,
        ("", "<w:tblStyle/>") ,
        ("<w:style w:type='paragraph' w:styleId='s'/>", "<w:tblStyle w:val='s'/>") ,
        ("<w:style w:type='table' w:styleId='s'><w:basedOn w:val='missing'/></w:style>", "<w:tblStyle w:val='s'/>") ,
        ("<w:style w:type='table' w:styleId='s'><w:basedOn/></w:style>", "<w:tblStyle w:val='s'/>") ,
        ("<w:style w:type='paragraph' w:styleId='p'/><w:style w:type='table' w:styleId='s'><w:basedOn w:val='p'/></w:style>", "<w:tblStyle w:val='s'/>") ,
        ("<w:style w:type='table' w:styleId='s'><w:basedOn w:val='s'/></w:style>", "<w:tblStyle w:val='s'/>") ,
    ] {
        let d = read(styles, &one(&format!("{}{selector}", borders("single", 4)), ""));
        assert_admission(&d, None, Some("unsupported_source_format"));
        assert_admission(&read(styles, &one(&borders("single", 4), "")), Some(50), None);
    }
    let styles = "<w:style w:type='table' w:default='1' w:styleId='base'/><w:style w:type='table' w:styleId='s'><w:basedOn w:val='base'/></w:style>";
    assert_admission(
        &read(styles, &one(&format!("{}<w:tblStyle w:val='s'/>", borders("single", 4)), "")),
        Some(50),
        None,
    );
    assert_admission(
        &read(styles, &one(&format!("{}<w:tblStyle w:val='missing'/>", borders("single", 4)), "")),
        None,
        Some("unsupported_source_format"),
    );
}

// 이것을 실패시키는 것: 잘못된 병합/격자 수치를 기본값으로 수용하여 선 경계를 바꿈.
#[test]
fn invalid_merge_and_grid_counts_remain_errors() {
    for cp in [
        "<w:vMerge w:val='future'/>",
        "<w:gridSpan/>",
        "<w:gridSpan w:val='0'/>",
        "<w:gridSpan w:val='-1'/>",
        "<w:gridSpan w:val='bad'/>",
    ] {
        assert!(decode_docx(&package(&one(&borders("single", 4), cp), &[])).is_err());
    }
}

// 이것을 실패시키는 것: 중첩 배치의 맞춤/들여쓰기 생략과 없는 첫 칸의 여백 생략을 수용함.
#[test]
fn placement_fallbacks_record_local_source_uncertainty() {
    for mode in ["14", "15", "12"] {
        let settings = format!("<w:settings xmlns:w='{W}'><w:compat><w:compatSetting w:name='compatibilityMode' w:uri='http://schemas.microsoft.com/office/word' w:val='{mode}'/></w:compat></w:settings>");
        for (pr, admit) in [
            ("<w:jc w:val='center'/>", false),
            ("<w:jc w:val='right'/>", false),
            ("<w:tblInd w:w='20'/>", false),
            ("<w:tblInd w:w='-20'/>", false),
            ("<w:jc w:val='left'/><w:tblInd w:w='0'/>", true),
        ] {
            let nested = one(&format!("{}{pr}", borders("single", 4)), "");
            let body = table(&borders("single", 4), &format!("<w:tr>{}</w:tr>", cell("", &nested)));
            let d =
                decode_docx(&package(&body, &[("settings", "settings.xml", &settings)])).unwrap();
            let ts = tables(&d);
            assert_eq!(widths(&d, &ts[0]), vec![[Some(50); 4]; 2]);
            assert_eq!(widths(&d, &ts[1]), vec![[admit.then_some(50); 4]; 2]);
            assert_eq!(warnings(&d), u64::from(!admit));
        }
        let body = table(
            &borders("single", 4),
            &format!("<w:tr><w:trPr><w:gridBefore w:val='1'/></w:trPr>{}</w:tr>", cell("", "")),
        );
        let d = decode_docx(&package(&body, &[("settings", "settings.xml", &settings)])).unwrap();
        let ts = tables(&d);
        // The inferred leading cell already has no representable source face.
        assert_eq!(warnings(&d), 1);
        assert!(widths(&d, &ts[0]).iter().flatten().all(Option::is_none));
        if mode == "14" {
            assert!(d.warnings.iter().any(|w| w.loss() == "table_border_space"
                && w.message().contains("unsupported_source_format")));
        }
    }
}

// 이것을 실패시키는 것: 누락 크기를 기본 폭으로 확정하거나 범위 밖 원본 수치를 그대로 수용.
#[test]
fn unproven_border_sizes_defer_before_defaulting() {
    for size in ["", " w:sz='1'", " w:sz='97'"] {
        let border = format!("<w:left w:val='single'{size}/>");
        for owner in ["table", "cell", "style"] {
            let styles = if owner == "style" {
                format!("<w:style w:type='table' w:styleId='s'><w:tblPr><w:tblBorders>{border}</w:tblBorders></w:tblPr></w:style>")
            } else {
                String::new()
            };
            let pr = match owner {
                "table" => format!("<w:tblBorders>{border}</w:tblBorders>"),
                "style" => "<w:tblStyle w:val='s'/>".into(),
                _ => String::new(),
            };
            let cp = if owner == "cell" {
                format!("<w:tcBorders>{border}</w:tcBorders>")
            } else {
                String::new()
            };
            assert_admission(
                &read(&styles, &one(&format!("{}{pr}", borders("single", 4)), &cp)),
                None,
                Some("unsupported_source_border"),
            );
        }
    }
    for (size, expected) in [(2, 25), (4, 50), (96, 1200)] {
        let d = read(
            "",
            &one(
                &format!(
                    "{}<w:tblBorders><w:left w:val='single' w:sz='{size}'/></w:tblBorders>",
                    borders("single", 4)
                ),
                "",
            ),
        );
        assert_eq!(
            widths(&d, &tables(&d)[0]),
            vec![[Some(expected), Some(50), Some(50), Some(50)]; 2]
        );
        assert_eq!(warnings(&d), 0);
    }
    for line in ["none", "nil"] {
        for size in ["", " w:sz='1'", " w:sz='97'"] {
            let cp = format!("<w:tcBorders><w:insideH w:val='{line}'{size}/></w:tcBorders>");
            assert_admission(&read("", &one(&borders("single", 4), &cp)), Some(50), None);
        }
    }
}

// 이것을 실패시키는 것: 이미 보류하는 비절대 여백을 기본값으로 바꾼 뒤 폭을 수용함.
#[test]
fn existing_margin_value_guards_keep_literal_admission() {
    for unit in ["pct", "auto", "nil", "future"] {
        for cell_margin in [false, true] {
            let margin = format!("<w:left w:w='20' w:type='{unit}'/>");
            let pr = if cell_margin {
                String::new()
            } else {
                format!("<w:tblCellMar>{margin}</w:tblCellMar>")
            };
            let cp =
                if cell_margin { format!("<w:tcMar>{margin}</w:tcMar>") } else { String::new() };
            assert_admission(
                &read("", &one(&format!("{}{pr}", borders("single", 4)), &cp)),
                None,
                Some("unsupported_source_format"),
            );
        }
    }
    assert_admission(
        &read(
            "",
            &one(&borders("single", 4), "<w:tcMar><w:left w:w='108' w:type='dxa'/></w:tcMar>"),
        ),
        Some(50),
        None,
    );
}
