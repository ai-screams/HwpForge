use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};
use hwpforge_core::table::Table;

fn read(mode: &str, properties: &str) -> DocxDecoded {
    let settings = format!("<w:settings xmlns:w='{W}'><w:compat><w:compatSetting w:name='compatibilityMode' w:uri='http://schemas.microsoft.com/office/word' w:val='{mode}'/></w:compat></w:settings>");
    let body = format!("<w:tbl><w:tblPr>{properties}</w:tblPr><w:tblGrid><w:gridCol w:w='2000'/></w:tblGrid><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>");
    decode_docx(&package(&body, &[("settings", "settings.xml", &settings)])).unwrap()
}
fn tables(d: &DocxDecoded) -> Vec<Table> {
    let mut result = Vec::new();
    d.document.for_each_table(|t| result.push(t.clone()));
    result
}
// 이것을 실패시키는 것: compatibilityMode 해석을 생략하거나 기본 위치에도 Some 생성.
#[test]
fn table_horizontal_placement_modes() {
    assert!(tables(&read("15", ""))[0].placement.is_none());
    assert_eq!(tables(&read("14", ""))[0].placement.as_ref().unwrap().horz_offset.as_i32(), -540);
}
// 이것을 실패시키는 것: 음수 들여쓰기를 0으로 제한하거나 twip 배율 제거.
#[test]
fn table_horizontal_placement_signed_indent() {
    for (mode, indent, expected) in
        [("15", "567", 2835), ("14", "567", 2295), ("15", "-283", -1415), ("14", "-283", -1955)]
    {
        let d = read(mode, &format!("<w:tblInd w:w='{indent}' w:type='dxa'/>"));
        assert_eq!(tables(&d)[0].placement.as_ref().unwrap().horz_offset.as_i32(), expected);
    }
}

fn table(properties: &str, row: &str) -> String {
    format!("<w:tbl><w:tblPr>{properties}</w:tblPr><w:tblGrid><w:gridCol w:w='2000'/><w:gridCol w:w='2000'/></w:tblGrid><w:tr>{row}</w:tr></w:tbl>")
}
fn decode(body: &str, declarations: &str, styles: &str) -> DocxDecoded {
    let settings = format!("<w:settings xmlns:w='{W}' xmlns:x='urn:foreign'><w:compat>{declarations}</w:compat></w:settings>");
    let styles = format!("<w:styles xmlns:w='{W}'>{styles}</w:styles>");
    decode_docx(&package(
        body,
        &[("settings", "settings.xml", &settings), ("styles", "styles.xml", &styles)],
    ))
    .unwrap()
}
fn mode(value: &str) -> String {
    format!("<w:compatSetting w:name='compatibilityMode' w:uri='http://schemas.microsoft.com/office/word' w:val='{value}'/>")
}
fn loss(d: &DocxDecoded, name: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == name).map(|w| w.count()).sum()
}
fn position(t: &Table) -> (&str, i32) {
    t.placement
        .as_ref()
        .map_or(("LEFT", 0), |p| (p.horz_align.as_hwpx_str(), p.horz_offset.as_i32()))
}
// 이것을 실패시키는 것: CENTER에 들여쓰기 적용, RIGHT offset 부호 반전, start/end 제외.
#[test]
fn table_horizontal_placement_alignment() {
    for (mode, alignment, indent, expected, warning) in [
        ("14", "center", "567", ("CENTER", 0), 0),
        ("15", "right", "0", ("RIGHT", 0), 0),
        ("14", "right", "0", ("RIGHT", -540), 0),
        ("14", "right", "567", ("RIGHT", -540), 1),
        ("15", "right", "-283", ("RIGHT", 0), 1),
        ("14", "start", "0", ("LEFT", -540), 0),
        ("15", "end", "0", ("RIGHT", 0), 0),
    ] {
        let d = read(mode, &format!("<w:jc w:val='{alignment}'/><w:tblInd w:w='{indent}'/>"));
        assert_eq!(position(&tables(&d)[0]), expected);
        assert_eq!(loss(&d, "table_indent_alignment"), warning);
    }
}
// 이것을 실패시키는 것: 첫 칸 override 무시, 오른쪽 여백을 위치 계산에 사용, gridBefore 검사 제거.
#[test]
fn table_horizontal_placement_first_original_margin() {
    for (align, left, expected) in [("left", "283", -1415), ("right", "280", -1400)] {
        for right in ["0", "600"] {
            let row = format!("<w:tc><w:tcPr><w:tcMar><w:left w:w='{left}'/><w:right w:w='{right}'/></w:tcMar></w:tcPr><w:p/></w:tc><w:tc><w:tcPr><w:tcMar><w:right w:w='900'/></w:tcMar></w:tcPr><w:p/></w:tc>");
            let d = decode(&table(&format!("<w:jc w:val='{align}'/><w:tblCellMar><w:left w:w='10'/><w:right w:w='1000'/></w:tblCellMar>"), &row), &mode("14"), "");
            assert_eq!(position(&tables(&d)[0]).1, expected);
        }
    }
    for (align, expected) in [("left", ("LEFT", 2835)), ("right", ("RIGHT", 0))] {
        let d = decode(
            &table(
                &format!("<w:jc w:val='{align}'/><w:tblInd w:w='567'/>"),
                "<w:trPr><w:gridBefore w:val='1'/></w:trPr><w:tc><w:p/></w:tc>",
            ),
            &mode("14"),
            "",
        );
        assert_eq!(position(&tables(&d)[0]), expected);
        assert_eq!(loss(&d, "table_legacy_margin"), 1);
    }
}
// 이것을 실패시키는 것: 중첩 예외 뒤에 정렬 계산, 양수/음수 들여쓰기 진단 제거, 경고 중복.
#[test]
fn table_horizontal_placement_nested_fallback() {
    for compat in ["14", "15"] {
        for (align, indent, warning) in [
            ("left", "0", 0),
            ("center", "0", 1),
            ("right", "0", 1),
            ("left", "567", 1),
            ("left", "-283", 1),
            ("right", "567", 1),
        ] {
            let inner = table(
                &format!("<w:jc w:val='{align}'/><w:tblInd w:w='{indent}'/>"),
                "<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc>",
            );
            let d = decode(
                &table("", &format!("<w:tc>{inner}</w:tc><w:tc><w:p/></w:tc>")),
                &mode(compat),
                "",
            );
            let ts = tables(&d);
            assert_eq!(position(&ts[0]), ("LEFT", if compat == "14" { -540 } else { 0 }));
            assert!(ts[1].placement.is_none());
            assert_eq!(loss(&d, "nested_table_placement"), warning);
            assert_eq!(loss(&d, "table_indent_alignment"), 0);
        }
    }
}
// 이것을 실패시키는 것: 누락을 15로 처리, 범위 검사 제거, 마지막 유효 정수/중복 진단 제거.
#[test]
fn table_horizontal_placement_mode_selection() {
    let body = table("<w:tblInd w:w='567'/>", "<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc>");
    for (values, expected, count, action) in [
        (vec![], 0, 1, "retain_placement"),
        (vec!["13"], 0, 1, "retain_placement"),
        (vec!["16"], 0, 1, "retain_placement"),
        (vec!["invalid"], 0, 1, "retain_placement"),
        (vec!["14", "15"], 2835, 1, "use_last_valid"),
        (vec!["15", "14"], 2295, 1, "use_last_valid"),
        (vec!["14", "invalid"], 2295, 1, "use_last_valid"),
        (vec!["14", "16"], 0, 1, "retain_placement"),
    ] {
        let declarations = values.iter().map(|v| mode(v)).collect::<String>();
        let d = decode(&body, &declarations, "");
        assert_eq!(position(&tables(&d)[0]).1, expected);
        assert_eq!(loss(&d, "table_compatibility_mode"), count);
        assert_eq!(d.warnings.last().unwrap().action(), action);
    }
    let d = decode(&format!("{body}{body}"), "", "");
    assert_eq!(loss(&d, "table_compatibility_mode"), 1);
    let d = decode("<w:p/>", "", "");
    assert_eq!(loss(&d, "table_compatibility_mode"), 0);
    let d = decode("<w:p/>", &mode("13"), "");
    assert_eq!(loss(&d, "table_compatibility_mode"), 1);
}
// 이것을 실패시키는 것: namespace/URI/직접 부모 검사 제거, 보조 part 표를 본문 표로 계산.
#[test]
fn table_horizontal_placement_setting_scope() {
    let body = table("<w:jc w:val='right'/>", "<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc>");
    for declaration in [
        mode("14").replace("w:compatSetting", "x:compatSetting"),
        mode("14").replace("http://schemas.microsoft.com/office/word", "urn:foreign"),
        format!("<w:other>{}</w:other>", mode("14")),
    ] {
        let d = decode(&body, &declaration, "");
        assert!(tables(&d)[0].placement.is_none());
        assert_eq!(loss(&d, "table_compatibility_mode"), 1);
    }
    let settings = format!("<w:wrong xmlns:w='{W}'><w:compat>{}</w:compat></w:wrong>", mode("14"));
    let d = decode_docx(&package(&body, &[("settings", "settings.xml", &settings)])).unwrap();
    assert!(tables(&d)[0].placement.is_none());
    let header = format!("<w:hdr xmlns:w='{W}'>{body}</w:hdr>");
    let d = decode_docx(&package("<w:p/>", &[("header", "header.xml", &header)])).unwrap();
    assert_eq!(loss(&d, "table_compatibility_mode"), 0);
    let d = decode(
        &format!("<w:sdt><w:sdtPr>{body}</w:sdtPr><w:sdtContent><w:p/></w:sdtContent></w:sdt>"),
        "",
        "",
    );
    assert_eq!(loss(&d, "table_compatibility_mode"), 0);
}
// 이것을 실패시키는 것: 명시 0을 상속으로 취급, bidi 지원으로 오인, 범위 검사 제거.
#[test]
fn table_horizontal_placement_inheritance_and_direction() {
    let styles = "<w:style w:type='table' w:default='1' w:styleId='base'><w:tblPr><w:tblInd w:w='567'/><w:jc w:val='right'/><w:bidiVisual/></w:tblPr></w:style>";
    let body = table(
        "<w:tblInd w:w='0'/><w:jc w:val='left'/><w:bidiVisual w:val='0'/>",
        "<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc>",
    );
    let d = decode(&body, &mode("15"), styles);
    assert!(tables(&d)[0].placement.is_none());
    assert_eq!(loss(&d, "table_alignment_direction"), 0);
    for properties in ["<w:bidiVisual/>", "<w:jc w:val='unknown'/>"] {
        let d = read("14", properties);
        assert!(tables(&d)[0].placement.is_none());
        assert_eq!(loss(&d, "table_alignment_direction"), 1);
    }
    for value in ["-20000000.01", "20000000.01", "bad"] {
        let body =
            table(&format!("<w:tblInd w:w='{value}'/>"), "<w:tc><w:p/></w:tc><w:tc><w:p/></w:tc>");
        assert!(decode_docx(&package(&body, &[])).is_err());
    }
    assert_eq!(position(&tables(&read("15", "<w:tblInd w:w='-14.15pt'/>"))[0]).1, -1415);
}

// 이것을 실패시키는 것: 행 맞춤의 val을 지원 속성으로 처리하여 기존 경고를 제거.
#[test]
fn table_horizontal_placement_row_alignment_attributes() {
    let row = "<w:trPr><w:jc w:val='center'/><w:jc w:val='right'/><w:jc w:val='left'/></w:trPr><w:tc><w:p/></w:tc>";
    let d = decode(&table("", row), &mode("15"), "");
    assert_eq!(loss(&d, "table_attribute"), 3);
    assert_eq!(loss(&d, "table_property"), 3);
    assert!(tables(&d)[0].placement.is_none());
}

// 이것을 실패시키는 것: 표 맞춤의 지원 속성을 미지원으로 진단하거나 스타일 문맥 누락.
#[test]
fn table_horizontal_placement_table_alignment_attributes() {
    let d = read("15", "<w:jc w:val='center'/>");
    assert_eq!(loss(&d, "table_attribute"), 0);
    assert_eq!(loss(&d, "table_property"), 0);
    assert_eq!(position(&tables(&d)[0]), ("CENTER", 0));
    let styles = "<w:style w:type='table' w:default='1' w:styleId='base'><w:tblPr><w:jc w:val='right'/></w:tblPr></w:style>";
    let d = decode(&table("", "<w:tc><w:p/></w:tc>"), &mode("15"), styles);
    assert_eq!(loss(&d, "table_attribute"), 0);
    assert_eq!(loss(&d, "table_property"), 0);
    assert_eq!(position(&tables(&d)[0]), ("RIGHT", 0));
}

// 이것을 실패시키는 것: 칸 문맥의 표 위치 속성을 해석하여 기존 미지원 경고를 제거.
#[test]
fn table_horizontal_placement_cell_property_scope() {
    for (property, attributes) in [
        ("<w:tblInd w:w='567'/>", 0),
        ("<w:tblInd w:w='invalid'/>", 0),
        ("<w:jc w:val='center'/>", 1),
        ("<w:bidiVisual w:val='1'/>", 1),
    ] {
        let row = format!("<w:tc><w:tcPr>{property}</w:tcPr><w:p/></w:tc>");
        let d = decode(&table("", &row), &mode("15"), "");
        assert_eq!(loss(&d, "table_property"), 1, "{property}");
        assert_eq!(loss(&d, "table_attribute"), attributes, "{property}");
        assert!(tables(&d)[0].placement.is_none());
    }
}
