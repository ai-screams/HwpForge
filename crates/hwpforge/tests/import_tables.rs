#![cfg(feature = "ops-docx")]

use hwpforge::ops::convert_docx;
#[path = "support/docx.rs"]
mod docx;
#[path = "support/xml.rs"]
mod xml;
use std::io::{Cursor, Read};
fn trees(bytes: Vec<u8>) -> (xml::Element, xml::Element) {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut read = |name: &str| {
        let mut s = String::new();
        archive.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        xml::Element::parse(&s)
    };
    (read("Contents/header.xml"), read("Contents/section0.xml"))
}
// 이것을 실패시키는 것: import 생성자를 legacy로 바꿈, 병합 좌표/폭/테두리 wire 값 변경.
#[test]
fn imported_grid_has_literal_coordinates_and_border_definitions() {
    let input=docx::package("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='20'/><w:tblCellMar><w:left w:w='0'/><w:right w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:tc><w:tcPr><w:gridSpan w:val='2'/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr></w:tbl>");
    let result = convert_docx(&input).unwrap();
    let (header, section) = trees(result.bytes);
    let table = section.find("hp:tbl");
    assert_eq!(table.attr("rowCnt"), Some("2"));
    assert_eq!(table.attr("colCnt"), Some("2"));
    let cells = table.descendants("hp:tc");
    assert_eq!(cells.len(), 3);
    for (cell, (r, c, span, width)) in
        cells.iter().zip([("0", "0", "2", "100"), ("1", "0", "1", "50"), ("1", "1", "1", "50")])
    {
        assert_eq!(cell.child("hp:cellAddr").attr("rowAddr"), Some(r));
        assert_eq!(cell.child("hp:cellAddr").attr("colAddr"), Some(c));
        assert_eq!(cell.child("hp:cellSpan").attr("rowSpan"), Some("1"));
        assert_eq!(cell.child("hp:cellSpan").attr("colSpan"), Some(span));
        assert_eq!(cell.child("hp:cellSz").attr("width"), Some(width));
        assert_eq!(cell.attr("borderFillIDRef"), Some("4"));
    }
    let fills = header.descendants("hh:borderFill");
    assert_eq!(fills.len(), 4);
    let fill = fills[3];
    assert_eq!(fill.attr("id"), Some("4"));
    for side in ["hh:leftBorder", "hh:rightBorder", "hh:topBorder", "hh:bottomBorder"] {
        assert_eq!(fill.child(side).attr("type"), Some("NONE"));
    }
}
// 이것을 실패시키는 것: 셀 목록 누락/새 그룹 생성, 표 보유 문단에 표식 복제.
#[test]
fn cell_numbers_and_following_body_share_one_sequence() {
    let defs="<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>";
    let item = |text| {
        format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr><w:r><w:t>{text}</w:t></w:r></w:p>")
    };
    let body =
        format!("<w:tbl><w:tr><w:tc>{}{}</w:tc></w:tr></w:tbl>{}", item("A"), item("B"), item("C"));
    let (header, section) =
        trees(convert_docx(&docx::package_with_numbering(&body, defs)).unwrap().bytes);
    let shapes = header.descendants("hh:paraPr");
    let mut numbered = Vec::new();
    let host = &section.children[0];
    let mut paragraphs = host.find("hp:tbl").descendants("hp:p");
    paragraphs.push(host);
    for p in paragraphs {
        let id = p.attr("paraPrIDRef").unwrap();
        let shape = shapes.iter().find(|s| s.attr("id") == Some(id)).unwrap();
        let heading = shape.child("hh:heading");
        if heading.attr("type") == Some("NUMBER") {
            assert_eq!(heading.attr("idRef"), Some("2"));
            assert_eq!(heading.attr("level"), Some("0"));
            numbered.push(
                p.children
                    .iter()
                    .filter(|n| n.name == "hp:run")
                    .flat_map(|r| &r.children)
                    .filter(|n| n.name == "hp:t")
                    .map(|t| t.text.as_str())
                    .collect::<String>(),
            );
        }
    }
    assert_eq!(numbered, ["A", "B", "C"]);
    let definitions = header.descendants("hh:numbering");
    let sequence = definitions.iter().find(|n| n.attr("id") == Some("2")).unwrap();
    assert_eq!(sequence.attr("start"), Some("1"));
    assert_eq!(sequence.children[0].text, "^1.");
    // The built-in definition plus exactly one imported numbering definition.
    assert_eq!(
        definitions.iter().map(|n| n.attr("id")).collect::<Vec<_>>(),
        [Some("1"), Some("2")]
    );
}

// 이것을 실패시키는 것: 폭 계산에 여백 0 사용, 출력 기본 여백 사용, 비양수 내용 폭 허용.
#[test]
fn nested_width_and_wire_margins_use_the_same_word_defaults() {
    for (pr, expected, left, right) in [
        ("", "3920", "540", "540"),
        ("<w:tcMar><w:left w:w='20'/><w:right w:w='10'/></w:tcMar>", "4850", "100", "50"),
    ] {
        let body=format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='1000'/></w:tblPr><w:tr><w:tc><w:tcPr>{pr}</w:tcPr><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>");
        let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
        let tables = section.descendants("hp:tbl");
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[1].child("hp:sz").attr("width"), Some(expected));
        let cells = tables[0].descendants("hp:tc");
        for (side, value) in [("left", left), ("right", right), ("top", "0"), ("bottom", "0")] {
            assert_eq!(cells[0].child("hp:cellMargin").attr(side), Some(value));
        }
        assert_eq!(cells[1].child("hp:cellMargin").attr("left"), Some("540"));
    }
    for width in ["200", "216"] {
        let body=format!("<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='{width}'/></w:tblPr><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl>");
        assert!(convert_docx(&docx::package(&body)).is_err());
    }
}
// 이것을 실패시키는 것: auto의 val을 최소 높이로 출력, 행 최소 높이/세로 정렬 값 누락·교환.
#[test]
fn cell_height_and_alignment_reach_hwpx() {
    let mut cells = String::new();
    for align in ["", "top", "center", "bottom"] {
        let pr =
            if align.is_empty() { String::new() } else { format!("<w:vAlign w:val='{align}'/>") };
        cells.push_str(&format!("<w:tc><w:tcPr>{pr}</w:tcPr><w:p><w:pPr><w:spacing w:line='240'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p></w:tc>"));
    }
    for row_pr in ["", "<w:trPr><w:trHeight w:hRule='auto' w:val='2000'/></w:trPr>"] {
        let section = "<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>";
        let result = convert_docx(&docx::package_with_defaults(&format!(
            "<w:tbl><w:tblPr><w:tblW w:type='dxa' w:w='2000'/></w:tblPr><w:tblGrid><w:gridCol w:w='500'/><w:gridCol w:w='500'/><w:gridCol w:w='500'/><w:gridCol w:w='500'/></w:tblGrid><w:tr>{row_pr}{cells}</w:tr></w:tbl>{section}"
        ), ""))
        .unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        let (_, section) = trees(result.bytes);
        for (cell, align) in
            section.descendants("hp:tc").iter().zip(["TOP", "TOP", "CENTER", "BOTTOM"])
        {
            assert_eq!(cell.child("hp:cellSz").attr("height"), Some("1902"));
            assert_eq!(cell.child("hp:subList").attr("vertAlign"), Some(align));
        }
    }
}

// 이것을 실패시키는 것: 병합 시작 행에 구간 높이 부과, continuation-only 행 최소값 소실/구간 합 누락.
#[test]
fn merged_region_preserves_otherwise_unrepresented_row_minima() {
    for (other, expected) in [
        ("", "4000"),
        (
            "<w:tc><w:p><w:pPr><w:spacing w:line='100' w:lineRule='exact'/></w:pPr></w:p></w:tc>",
            "1000",
        ),
    ] {
        let body = format!("<w:tbl><w:tr><w:trPr><w:trHeight w:val='200'/></w:trPr><w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr><w:p><w:pPr><w:spacing w:line='0' w:lineRule='exact'/></w:pPr></w:p></w:tc>{other}</w:tr><w:tr><w:trPr><w:trHeight w:val='600'/></w:trPr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{other}</w:tr></w:tbl>");
        let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
        let cells = section.descendants("hp:tc");
        assert_eq!(cells[0].child("hp:cellSpan").attr("rowSpan"), Some("2"));
        assert_eq!(cells[0].child("hp:cellSz").attr("height"), Some(expected));
        if !other.is_empty() {
            assert_eq!(cells[1].child("hp:cellSz").attr("height"), Some("1000"));
            assert_eq!(cells[2].child("hp:cellSz").attr("height"), Some("3000"));
        }
    }
}

// 이것을 실패시키는 것: 병합 anchor 의 내용 최소를 구간 행 합과의 max 에서 빼기,
// 내용이 큰 anchor 가 있는 시작 행의 일반 셀 최소값 명시 제거.
#[test]
fn merged_anchor_content_above_row_minima_sets_the_region_height() {
    for (other, anchor) in [
        ("", "6000"),
        (
            "<w:tc><w:p><w:pPr><w:spacing w:line='100' w:lineRule='exact'/></w:pPr></w:p></w:tc>",
            "6000",
        ),
    ] {
        let body = format!("<w:tbl><w:tr><w:trPr><w:trHeight w:val='200'/></w:trPr><w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr><w:p><w:pPr><w:spacing w:line='1200' w:lineRule='exact'/></w:pPr></w:p></w:tc>{other}</w:tr><w:tr><w:trPr><w:trHeight w:val='600'/></w:trPr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{other}</w:tr></w:tbl>");
        let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
        let cells = section.descendants("hp:tc");
        assert_eq!(cells[0].child("hp:cellSpan").attr("rowSpan"), Some("2"));
        assert_eq!(cells[0].child("hp:cellSz").attr("height"), Some(anchor));
        if !other.is_empty() {
            assert_eq!(cells[1].child("hp:cellSz").attr("height"), Some("1000"));
            assert_eq!(cells[2].child("hp:cellSz").attr("height"), Some("3000"));
        }
    }
}
// 이것을 실패시키는 것: 병합 anchor에 높이를 명시한 행의 나머지 시작 셀 최소값 보존 제거.
#[test]
fn crossing_merges_preserve_the_starting_rows_ordinary_cell_minimum() {
    let paragraph = "<w:p><w:pPr><w:spacing w:line='0' w:lineRule='exact'/></w:pPr></w:p>";
    let cell = |merge| format!("<w:tc><w:tcPr>{merge}</w:tcPr>{paragraph}</w:tc>");
    let restart = cell("<w:vMerge w:val='restart'/>");
    let continuation = cell("<w:vMerge/>");
    let ordinary = cell("");
    let body = format!(
        "<w:tbl><w:tblPr><w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:trPr><w:trHeight w:val='200' w:hRule='atLeast'/></w:trPr>{restart}{ordinary}</w:tr><w:tr><w:trPr><w:trHeight w:val='600' w:hRule='atLeast'/></w:trPr>{continuation}{restart}</w:tr><w:tr><w:trPr><w:trHeight w:val='1000' w:hRule='atLeast'/></w:trPr>{continuation}{continuation}</w:tr></w:tbl>"
    );
    let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
    let cells = section.find("hp:tbl").descendants("hp:tc");
    assert_eq!(cells.len(), 3);
    for (cell, (row, col, span, height)) in cells.iter().zip([
        ("0", "0", "3", "9000"),
        ("0", "1", "1", "1000"),
        ("1", "1", "2", "8000"),
    ]) {
        assert_eq!(cell.child("hp:cellAddr").attr("rowAddr"), Some(row));
        assert_eq!(cell.child("hp:cellAddr").attr("colAddr"), Some(col));
        assert_eq!(cell.child("hp:cellSpan").attr("rowSpan"), Some(span));
        assert_eq!(cell.child("hp:cellSz").attr("height"), Some(height));
    }
}
// 이것을 실패시키는 것: 병합 없는 행의 기존 row.height fallback 출력 변경.
#[test]
fn unmerged_cells_keep_literal_row_minima() {
    let cell = "<w:tc><w:p><w:pPr><w:spacing w:line='0' w:lineRule='exact'/></w:pPr></w:p></w:tc>";
    let body = format!(
        "<w:tbl><w:tr><w:trPr><w:trHeight w:val='200'/></w:trPr>{cell}{cell}</w:tr><w:tr><w:trPr><w:trHeight w:val='600'/></w:trPr>{cell}{cell}</w:tr><w:tr><w:trPr><w:trHeight w:val='1000'/></w:trPr>{cell}{cell}</w:tr></w:tbl>"
    );
    let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
    let cells = section.find("hp:tbl").descendants("hp:tc");
    assert_eq!(cells.len(), 6);
    for (cell, height) in cells.iter().zip(["1000", "1000", "3000", "3000", "5000", "5000"]) {
        assert_eq!(cell.child("hp:cellSpan").attr("rowSpan"), Some("1"));
        assert_eq!(cell.child("hp:cellSz").attr("height"), Some(height));
    }
}
// 이것을 실패시키는 것: 본문/중첩 표 out_margin 생략 또는 0 아닌 여백 사용, 여러 문단 하한 누락.
#[test]
fn inline_tables_have_zero_outer_margins_and_multi_paragraph_height() {
    let p = "<w:p><w:pPr><w:spacing w:line='240' w:after='120'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p>";
    let nested = format!("<w:tbl><w:tr><w:tc>{p}{p}</w:tc></w:tr></w:tbl>");
    let body = format!("<w:tbl><w:tr><w:tc>{nested}</w:tc><w:tc>{p}{p}</w:tc></w:tr></w:tbl>");
    let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
    let tables = section.descendants("hp:tbl");
    assert_eq!(tables.len(), 2);
    for t in tables {
        for side in ["left", "right", "top", "bottom"] {
            assert_eq!(t.child("hp:outMargin").attr(side), Some("0"));
        }
    }
    for cell in section.descendants("hp:tc") {
        assert_eq!(cell.child("hp:cellSz").attr("height"), Some("5004"));
    }
}

// 이것을 실패시키는 것: 마지막 문단 뒤 간격의 셀 하한을 wire cellSz height에 전달하지 않음.
#[test]
fn cell_edge_spacing_reaches_literal_wire_height() {
    let p = "<w:p><w:pPr><w:spacing w:before='0' w:after='200' w:line='276' w:lineRule='auto'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p>";
    let body = format!("<w:tbl><w:tr><w:tc>{p}</w:tc></w:tr></w:tbl>");
    let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
    assert_eq!(section.find("hp:tc").child("hp:cellSz").attr("height"), Some("3187"));
}

// 이것을 실패시키는 것: 강제 줄 높이 합을 wire cellSz height에 전달하지 않거나 br/cr 경계 누락.
#[test]
fn forced_line_sum_reaches_literal_wire_cell_height() {
    let p = "<w:p><w:pPr><w:spacing w:before='0' w:after='0' w:line='240' w:lineRule='auto'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한</w:t><w:br/><w:t>글</w:t><w:cr/><w:t>줄</w:t></w:r></w:p>";
    let body = format!("<w:tbl><w:tr><w:tc>{p}</w:tc></w:tr></w:tbl>");
    let (_, section) = trees(convert_docx(&docx::package_with_defaults(&body, "")).unwrap().bytes);
    assert_eq!(section.find("hp:tc").child("hp:cellSz").attr("height"), Some("5706"));
}

// 이것을 실패시키는 것: 병합 영역 max에서 내용 최소를 빼거나 일반 셀과 병합 셀의 내용 하한을 다르게 출력.
#[test]
fn merged_content_minimum_reaches_wire_without_row_heights() {
    let p = "<w:p><w:pPr><w:spacing w:before='0' w:after='0' w:line='240' w:lineRule='auto'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>가</w:t><w:br/><w:t>나</w:t><w:br/><w:t>다</w:t></w:r></w:p>";
    for (row_pr, expected) in [("", "5706"), ("<w:trPr><w:trHeight w:val='600'/></w:trPr>", "6000")]
    {
        let body = format!("<w:tbl><w:tblPr><w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr>{row_pr}<w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr>{p}</w:tc></w:tr><w:tr>{row_pr}<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc>{p}</w:tc></w:tr></w:tbl>");
        let (_, section) =
            trees(convert_docx(&docx::package_with_defaults(&body, "")).unwrap().bytes);
        let cells = section.descendants("hp:tc");
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].child("hp:cellSpan").attr("rowSpan"), Some("2"));
        assert_eq!(cells[0].child("hp:cellSz").attr("height"), Some(expected));
        assert_eq!(cells[1].child("hp:cellSpan").attr("rowSpan"), Some("1"));
        assert_eq!(cells[1].child("hp:cellSz").attr("height"), Some("5706"));
    }
}

// 이것을 실패시키는 것: 병합 영역 max에서 내용 최소를 빼거나 같은 시작 행의 일반 셀 wire 높이를 누락.
#[test]
fn merged_content_minimum_preserves_ordinary_wire_height() {
    let p = "<w:p><w:pPr><w:spacing w:before='0' w:after='0' w:line='240' w:lineRule='auto'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>가</w:t><w:br/><w:t>나</w:t><w:br/><w:t>다</w:t></w:r></w:p>";
    let ordinary =
        format!("<w:tc>{}</w:tc>", p.replace("<w:br/><w:t>나</w:t><w:br/><w:t>다</w:t>", ""));
    let body = format!("<w:tbl><w:tblPr><w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:tc><w:tcPr><w:vMerge w:val='restart'/></w:tcPr>{p}</w:tc>{ordinary}</w:tr><w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>{ordinary}</w:tr></w:tbl>");
    let (_, section) = trees(convert_docx(&docx::package_with_defaults(&body, "")).unwrap().bytes);
    let cells = section.descendants("hp:tc");
    assert_eq!(cells.len(), 3);
    for (cell, (row, col, span, height)) in cells.iter().zip([
        ("0", "0", "2", "5706"),
        ("0", "1", "1", "1902"),
        ("1", "1", "1", "1902"),
    ]) {
        assert_eq!(cell.child("hp:cellAddr").attr("rowAddr"), Some(row));
        assert_eq!(cell.child("hp:cellAddr").attr("colAddr"), Some(col));
        assert_eq!(cell.child("hp:cellSpan").attr("rowSpan"), Some(span));
        assert_eq!(cell.child("hp:cellSz").attr("height"), Some(height));
    }
}

#[test]
fn word_profile_auto_spacing_and_shared_anchor_reach_the_wire() {
    // 이것을 실패시키는 것: bridge의 프로필 유실, 표 순서/anchor 분리, auto 재환산, TOP 누락.
    for (line, value) in [(240, "100"), (276, "115"), (360, "150")] {
        let table = |text| {
            format!(
                "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"
            )
        };
        let body = format!("{}{}<w:p><w:pPr><w:spacing w:line='{line}' w:before='240' w:after='240'/></w:pPr><w:r><w:t>tail</w:t></w:r></w:p>", table("first"), table("second"));
        let result = convert_docx(&docx::package_with_defaults(&body, "")).unwrap();
        assert_eq!(result.paragraphs, 1);
        let (h, s) = trees(result.bytes);
        assert_eq!(h.find("hh:compatibleDocument").attr("targetProgram"), Some("MS_WORD"));
        assert_eq!(h.find("hh:layoutCompatibility").children.len(), 35);
        let ps: Vec<_> = s.children.iter().filter(|n| n.name == "hp:p").collect();
        assert_eq!(ps.len(), 1);
        let runs: Vec<_> = ps[0].children.iter().filter(|n| n.name == "hp:run").collect();
        assert_eq!(runs.len(), 3);
        assert_eq!(runs[0].find("hp:tbl").find("hp:t").text, "first");
        assert_eq!(runs[1].find("hp:tbl").find("hp:t").text, "second");
        assert_eq!(runs[2].find("hp:t").text, "tail");
        for cell in s.descendants("hp:tc") {
            assert_eq!(cell.child("hp:subList").attr("vertAlign"), Some("TOP"));
        }
        let shape = h
            .descendants("hh:paraPr")
            .into_iter()
            .find(|p| p.attr("id") == ps[0].attr("paraPrIDRef"))
            .unwrap();
        assert_eq!(shape.find("hh:lineSpacing").attr("type"), Some("PERCENT"));
        assert_eq!(shape.find("hh:lineSpacing").attr("value"), Some(value));
    }
}

// 이것을 실패시키는 것: 모든 행 exact 판정 대신 any 사용, exact에 최소·선 합산, 혼합 표 내용 최소 누락.
#[test]
fn exact_table_size_protection_and_literal_cell_heights_reach_hwpx() {
    let cell = "<w:tc><w:p><w:pPr><w:spacing w:line='240'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p></w:tc>";
    for (first, last, protect, heights) in [
        (
            "<w:trHeight w:val='200' w:hRule='exact'/>",
            "<w:trHeight w:val='300' w:hRule='exact'/>",
            "1",
            ["1000", "1000", "1500", "1500"],
        ),
        ("<w:trHeight w:val='200' w:hRule='exact'/>", "", "0", ["1952", "1952", "2002", "2002"]),
        ("", "<w:trHeight w:val='200' w:hRule='exact'/>", "0", ["1952", "1952", "2002", "2002"]),
        (
            "<w:trHeight w:val='200' w:hRule='exact'/>",
            "<w:trHeight w:hRule='exact'/>",
            "0",
            ["1952", "1952", "2002", "2002"],
        ),
        ("", "<w:trHeight w:val='200' w:hRule='atLeast'/>", "0", ["1952", "1952", "2002", "2002"]),
    ] {
        let body = format!("<w:tbl><w:tblPr><w:tblBorders><w:top w:val='single' w:sz='4'/><w:insideH w:val='single' w:sz='4'/><w:bottom w:val='single' w:sz='4'/></w:tblBorders><w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:trPr>{first}</w:trPr>{cell}{cell}</w:tr><w:tr><w:trPr>{last}</w:trPr>{cell}{cell}</w:tr></w:tbl>");
        let (_, section) = trees(convert_docx(&docx::package(&body)).unwrap().bytes);
        let table = section.find("hp:tbl");
        assert_eq!(table.child("hp:sz").attr("protect"), Some(protect));
        let cells = table.descendants("hp:tc");
        assert_eq!(cells.len(), 4);
        for (cell, height) in cells.iter().zip(heights) {
            assert_eq!(cell.child("hp:cellSz").attr("height"), Some(height));
        }
    }
}
