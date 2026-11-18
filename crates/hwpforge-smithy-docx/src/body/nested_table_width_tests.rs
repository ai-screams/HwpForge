use super::tests::package;
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::table::Table;
fn inner(kind: &str, value: &str, grid: &str, cell: &str) -> String {
    let width = if kind == "absent" {
        String::new()
    } else if kind == "default" {
        format!("<w:tblW w:w='{value}'/>")
    } else {
        format!("<w:tblW w:type='{kind}' w:w='{value}'/>")
    };
    format!("<w:tbl><w:tblPr>{width}</w:tblPr>{grid}<w:tr><w:tc><w:tcPr>{cell}</w:tcPr><w:p/></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl>")
}
fn grid(a: i32, b: i32) -> String {
    format!("<w:tblGrid><w:gridCol w:w='{a}'/><w:gridCol w:w='{b}'/></w:tblGrid>")
}
fn decode(inner: &str, nested: bool) -> DocxDecoded {
    let body = if nested {
        format!("<w:tbl><w:tblPr><w:tblW w:w='2268'/></w:tblPr><w:tblGrid><w:gridCol w:w='2268'/></w:tblGrid><w:tr><w:tc>{inner}</w:tc></w:tr></w:tbl>")
    } else {
        format!("{inner}<w:sectPr><w:pgSz w:w='2052' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>")
    };
    decode_docx(&package(&body, &[])).unwrap()
}
fn tables(d: &DocxDecoded) -> Vec<Table> {
    let mut ts = Vec::new();
    d.document.for_each_table(|t| ts.push(t.clone()));
    ts
}
fn count(d: &DocxDecoded, loss: &str, action: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == loss && w.action() == action).map(|w| w.count()).sum()
}
// 이것을 실패시키는 것: 완전 grid를 컨테이너에 제한, 작은 grid 확대, nil/미지정/type 누락 제외.
#[test]
fn nested_table_width_preserves_known_grid() {
    for (a, b, expected) in [
        (850, 850, 8500),
        (1134, 1134, 11340),
        (1701, 1701, 17010),
        (4418, 4418, 44180),
        (2152, 2153, 21525),
    ] {
        for kind in ["absent", "dxa", "auto", "nil", "default"] {
            let d = decode(&inner(kind, &(a + b).to_string(), &grid(a, b), ""), true);
            let ts = tables(&d);
            let t = &ts[1];
            assert_eq!(t.width.unwrap().as_i32(), expected, "{kind}/{a}");
            assert_eq!(t.rows[0].cells[0].width.as_i32(), a * 5);
            assert_eq!(t.rows[0].cells[1].width.as_i32(), b * 5);
            assert_eq!(count(&d, "table_width", "use_container"), 0);
            assert_eq!(count(&d, "table_width", "fit_columns"), 0);
            assert_eq!(count(&d, "table_width_conflict", "use_grid"), 0);
        }
    }
}
// 이것을 실패시키는 것: 미지 종류와 pct를 grid로 승격하거나 최상위에 중첩 정책 적용.
#[test]
fn nested_table_width_unknown_percentage_and_top_level() {
    for (kind, value, nested, container, unsupported) in [
        ("unknown", "3402", true, 0, 1),
        ("pct", "5000", true, 1, 0),
        ("pct", "2500", true, 1, 0),
        ("auto", "0", false, 1, 0),
    ] {
        let d = decode(&inner(kind, value, &grid(1701, 1701), ""), nested);
        let ts = tables(&d);
        let t = ts.last().unwrap();
        assert_eq!(t.width.unwrap().as_i32(), 10260);
        assert_eq!(t.rows[0].cells[0].width.as_i32(), 5130);
        assert_eq!(count(&d, "table_width", "fit_columns"), 1);
        assert_eq!(count(&d, "table_width", "use_container"), container);
        assert_eq!(count(&d, "table_width_type", "omit"), unsupported);
        assert_eq!(count(&d, "table_width_conflict", "use_grid"), 0);
        if unsupported == 1 {
            let w = d.warnings.iter().find(|w| w.loss() == "table_width_type").unwrap();
            assert_eq!(w.code(), "DOCX_UNSUPPORTED_ATTRIBUTE");
            assert_eq!(w.message(), "Unknown width type omitted; width uses the container");
            assert_eq!(w.part(), Some("text/body.xml"));
        }
    }
}
// 이것을 실패시키는 것: dxa 충돌 진단 제거/중복, tcW 충돌까지 숨김.
#[test]
fn nested_table_width_conflicts_are_distinct() {
    let d = decode(&inner("dxa", "2000", &grid(1134, 1134), ""), true);
    assert_eq!(tables(&d)[1].width.unwrap().as_i32(), 11340);
    assert_eq!(count(&d, "table_width_conflict", "use_grid"), 1);
    assert_eq!(count(&d, "table_width", "fit_columns"), 0);
    let d = decode(&inner("auto", "0", &grid(1134, 1134), "<w:tcW w:w='1000'/>"), true);
    assert_eq!(tables(&d)[1].rows[0].cells[0].width.as_i32(), 5670);
    assert_eq!(count(&d, "table_width", "use_container"), 0);
    assert_eq!(count(&d, "table_width", "fit_columns"), 1);
    assert_eq!(
        d.warnings.iter().find(|w| w.action() == "fit_columns").unwrap().message(),
        "Cell width preferences differ from preserved table grid widths"
    );
}
// 이것을 실패시키는 것: tcW로 보완한 weights를 원본 grid로 오인, 불완전 grid 진단 억제.
#[test]
fn nested_table_width_incomplete_grid_retains_approximation() {
    for (kind, g, cell, container) in [
        ("auto", "", "", 1),
        (
            "dxa",
            "<w:tblGrid><w:gridCol/><w:gridCol w:w='1701'/></w:tblGrid>",
            "<w:tcW w:w='1701'/>",
            0,
        ),
    ] {
        let d = decode(&inner(kind, "3402", g, cell), true);
        assert_eq!(tables(&d)[1].width.unwrap().as_i32(), 10260);
        assert_eq!(count(&d, "table_width", "use_container"), container);
        assert_eq!(count(&d, "table_width", "fit_columns"), 1);
    }
}

// 이것을 실패시키는 것: 지연된 tblW 경고를 tcW 뒤에 집계하거나 상속 사용 횟수만큼 복제.
#[test]
fn nested_table_width_deferred_diagnostics_preserve_source_order() {
    let body = "<w:tbl><w:tblPr><w:tblW w:type='auto'/><w:shd/></w:tblPr><w:tr><w:tc><w:tcPr><w:tcW w:type='auto'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>";
    let d = decode_docx(&package(body, &[])).unwrap();
    let width = d.warnings.iter().position(|w| w.action() == "use_container").unwrap();
    let shade = d.warnings.iter().position(|w| w.loss() == "cell_background").unwrap();
    assert!(width < shade);
    assert_eq!(d.warnings[width].count(), 2);
    assert_eq!(d.warnings[width].location(), Some("tblW#5"));
}

// 이것을 실패시키는 것: 선언 대신 표마다 진단, 직접 폭 overlay에서 종류/출처 누락, 미사용 선언 삭제.
#[test]
fn nested_table_width_inherited_declarations_are_settled_once() {
    let styles = format!("<w:styles xmlns:w='{}'><w:style w:type='table' w:styleId='s'><w:tblPr><w:tblW w:type='auto'/></w:tblPr></w:style></w:styles>", super::tests::W);
    let styled = inner("absent", "0", &grid(1134, 1134), "")
        .replace("<w:tblPr>", "<w:tblPr><w:tblStyle w:val='s'/>");
    let outer = |content: &str| {
        format!("<w:tbl><w:tblPr><w:tblW w:w='2268'/></w:tblPr><w:tr><w:tc>{content}</w:tc></w:tr></w:tbl>")
    };
    for (body, expected) in [
        (outer(&format!("{styled}{styled}")), 0),
        (format!("{styled}{}", outer(&styled)), 1),
        ("<w:p/>".to_string(), 1),
        (outer(&styled.replace("</w:tblPr>", "<w:tblW w:type='pct' w:w='5000'/></w:tblPr>")), 2),
    ] {
        let d = decode_docx(&package(&body, &[("styles", "styles.xml", &styles)])).unwrap();
        assert_eq!(count(&d, "table_width", "use_container"), expected);
        if expected == 2 {
            assert_eq!(tables(&d)[1].width.unwrap().as_i32(), 10260);
        }
        if expected > 0 {
            let w = d.warnings.iter().find(|w| w.action() == "use_container").unwrap();
            assert_eq!(w.part(), Some("text/styles.xml"));
        }
    }
}

// 이것을 실패시키는 것: 목록의 저장된 경고 위치를 소비하기 전에 폭 진단 슬롯을 제거/병합.
#[test]
fn nested_table_width_diagnostics_do_not_move_deferred_list_warnings() {
    let item = |text: &str| {
        format!("<w:p><w:pPr><w:numPr><w:ilvl w:val='0'/><w:numId w:val='1'/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
    };
    let numbering = format!("<w:numbering xmlns:w='{}'><w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl></w:abstractNum><w:num w:numId='1'><w:abstractNumId w:val='1'/></w:num></w:numbering>", super::tests::W);
    let nested = inner("auto", "0", &grid(1134, 1134), "")
        .replacen("<w:p/>", &item("b"), 1)
        .replacen("<w:p/>", &item("c"), 1);
    let body = format!("{}<w:tbl><w:tblPr><w:tblW w:w='2268'/></w:tblPr><w:tblGrid><w:gridCol w:w='2268'/></w:tblGrid><w:tr><w:tc>{nested}{}</w:tc></w:tr></w:tbl>{}<w:sectPr><w:docGrid w:type='lines'/></w:sectPr>",item("a"),item("d"),item("e"));
    let d = decode_docx(&package(&body, &[("numbering", "numbering.xml", &numbering)])).unwrap();
    let warnings = d
        .warnings
        .iter()
        .filter(|w| matches!(w.loss(), "list_numbering_split" | "page_property"))
        .map(|w| w.loss())
        .collect::<Vec<_>>();
    assert_eq!(warnings, ["list_numbering_split", "page_property"]);
    assert!(!d.warnings.iter().any(|w| w.loss() == "table_width"));
}

const CELL_WIDTH_BODIES: [(&str, &str, u64); 4] = [
    ("auto", "<w:tbl><w:tblPr><w:tblW w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tblW w:type='auto'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>", 1),
    ("nil", "<w:tbl><w:tblPr><w:tblW w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tblW w:type='nil'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>", 1),
    ("auto_with_cell_width", "<w:tbl><w:tblPr><w:tblW w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tblW w:type='auto'/><w:tcW w:type='auto'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>", 2),
    ("nil_with_cell_width", "<w:tbl><w:tblPr><w:tblW w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:tcPr><w:tblW w:type='nil'/><w:tcW w:type='auto'/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>", 2),
];

// 이것을 실패시키는 것: 칸 tblW를 표의 지연 경로로 보내거나 즉시 폭 진단을 누락.
#[test]
fn cell_table_width_warnings_preserve_first_location_and_count() {
    for (name, body, expected_count) in CELL_WIDTH_BODIES {
        let d = decode_docx(&package(body, &[])).unwrap();
        assert!(!d.warnings.iter().any(|w| w.action() == "pending_table_width"), "{name}");
        let warnings: Vec<_> = d.warnings.iter().filter(|w| w.loss() == "table_width").collect();
        assert_eq!(warnings.len(), 1, "{name}");
        let w = warnings[0];
        assert_eq!(
            (w.code(), w.loss(), w.action(), w.count(), w.part(), w.location()),
            (
                "DOCX_TABLE_WIDTH_APPROXIMATED",
                "table_width",
                "use_container",
                expected_count,
                Some("text/body.xml"),
                Some("tblW#11"),
            ),
            "{name}",
        );
    }
}

// 이것을 실패시키는 것: 본문 표의 읽기/수집 연결 또는 스타일 선언의 최종 확정 제거.
#[test]
fn table_width_declarations_are_finalized_in_each_context() {
    for kind in ["auto", "nil"] {
        for nested in [false, true] {
            let d = decode(&inner(kind, "0", &grid(1134, 1134), ""), nested);
            assert!(!d.warnings.iter().any(|w| w.action() == "pending_table_width"));
            assert_eq!(count(&d, "table_width", "use_container"), if nested { 0 } else { 1 });
        }
        for context in ["tblPr", "tcPr"] {
            let styles = format!("<w:styles xmlns:w='{}'><w:style w:type='table' w:styleId='T'><w:{context}><w:tblW w:type='{kind}'/></w:{context}></w:style></w:styles>", super::tests::W);
            let body = "<w:tbl><w:tblPr><w:tblStyle w:val='T'/></w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>";
            let d = decode_docx(&package(body, &[("styles", "styles.xml", &styles)])).unwrap();
            assert!(!d.warnings.iter().any(|w| w.action() == "pending_table_width"));
            assert_eq!(
                count(&d, "table_width", "use_container"),
                if context == "tblPr" { 1 } else { 0 }
            );
        }
    }
}
