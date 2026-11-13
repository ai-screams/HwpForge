#![cfg(feature = "ops-docx")]
use hwpforge::ops::{convert_docx, OpsWarning};
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;

fn input(marker: u32, font: &str, spacing: &str, bodies: &[u32]) -> Vec<u8> {
    let defs = format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}'/><w:sz w:val='{marker}'/></w:rPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
    let body: String = bodies.iter().map(|size| format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr>{spacing}<w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}'/><w:sz w:val='{size}'/></w:rPr></w:pPr><w:r><w:rPr><w:rFonts w:ascii='{font}' w:hAnsi='{font}' w:eastAsia='{font}'/><w:sz w:val='{size}'/></w:rPr><w:t>본문 body</w:t></w:r></w:p>")).collect();
    docx::package_with_numbering(&body, &defs)
}
fn check(marker: u32, font: &str, spacing: &str, expected: [&str; 2], action: Option<&str>) {
    check_input(&input(marker, font, spacing, &[22]), expected, action);
}
fn check_input(bytes: &[u8], expected: [&str; 2], action: Option<&str>) {
    let result = convert_docx(bytes).unwrap();
    let header = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    let section = xml::Element::parse(&package::section_xml(&result.bytes));
    let id = section.descendants("hp:p")[0].attr("paraPrIDRef").unwrap();
    let shape =
        header.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some(id)).unwrap();
    for (branch, expected) in ["hp:case", "hp:default"].into_iter().zip(expected) {
        let prev = shape.child("hp:switch").child(branch).child("hh:margin").child("hc:prev");
        assert_eq!(prev.attr("value"), Some(expected));
    }
    let warnings: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            OpsWarning::Docx(w) if w.loss() == "list_marker_line_height" => Some(w),
            _ => None,
        })
        .collect();
    if let Some(action) = action {
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].count(), 1);
        assert_eq!(warnings[0].action(), action);
        if action == "approximate_space_before" {
            assert_eq!(
                warnings[0].message(),
                "First-line height difference approximated with paragraph space before"
            );
        }
    } else {
        assert!(warnings.is_empty());
    }
}
// 이것을 실패시키는 것: 번호 높이 보정 생략.
#[test]
fn marker_height_single() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='240' w:lineRule='auto'/>",
        ["399", "798"],
        Some("approximate_space_before"),
    );
}
// 이것을 실패시키는 것: 같은 크기에도 번호 전체 높이를 앞 간격에 추가.
#[test]
fn marker_height_equal_keeps_existing_margin() {
    check(22, "맑은 고딕", "<w:spacing w:before='120'/>", ["600", "1200"], None);
}
// 이것을 실패시키는 것: exact를 auto로 계산.
#[test]
fn marker_height_exact() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='240' w:lineRule='exact'/>",
        ["0", "0"],
        Some("retain_body_line_height"),
    );
}
// 이것을 실패시키는 것: atLeast 최솟값을 무시.
#[test]
fn marker_height_large_minimum() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='480' w:lineRule='atLeast'/>",
        ["0", "0"],
        Some("retain_body_line_height"),
    );
}
// 이것을 실패시키는 것: auto 배수 360/240 적용 생략.
#[test]
fn marker_height_multiple() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='360' w:lineRule='auto'/>",
        ["599", "1198"],
        Some("approximate_space_before"),
    );
}
// 이것을 실패시키는 것: 기존 앞 간격을 덮어쓰기.
#[test]
fn marker_height_adds_to_original_before() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:before='120' w:line='240'/>",
        ["999", "1998"],
        Some("approximate_space_before"),
    );
}
// 이것을 실패시키는 것: 알 수 없는 metrics에 크기 차이로 보정 적용.
#[test]
fn marker_height_unknown_metrics() {
    check(
        28,
        "Unknown Test Face",
        "<w:spacing w:line='240'/>",
        ["0", "0"],
        Some("retain_body_line_height"),
    );
}
// 이것을 실패시키는 것: 본문 크기를 캐시 키에서 제외하거나 그룹당 첫 문단만 보정/매번 경고.
#[test]
fn marker_height_interns_by_effective_body_and_preserves_boundary() {
    let out = convert_docx(&input(28, "맑은 고딕", "", &[22, 22, 20])).unwrap();
    let section = xml::Element::parse(&package::section_xml(&out.bytes));
    let paragraphs = section.descendants("hp:p");
    assert_eq!(paragraphs[0].attr("paraPrIDRef"), paragraphs[1].attr("paraPrIDRef"));
    assert_ne!(paragraphs[0].attr("paraPrIDRef"), paragraphs[2].attr("paraPrIDRef"));
    let header = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    for (p, value) in paragraphs.iter().zip(["399", "399", "532"]) {
        let shape = header
            .descendants("hh:paraPr")
            .into_iter()
            .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
            .unwrap();
        assert_eq!(
            shape
                .child("hp:switch")
                .child("hp:case")
                .child("hh:margin")
                .child("hc:prev")
                .attr("value"),
            Some(value)
        );
    }
    let warnings: Vec<_> = out
        .warnings
        .iter()
        .filter_map(|w| match w {
            OpsWarning::Docx(w) if w.loss() == "list_marker_line_height" => Some(w.count()),
            _ => None,
        })
        .collect();
    assert_eq!(warnings, [1]);
}
// 이것을 실패시키는 것: atLeast에서 본문 자연 높이 대신 지정 최솟값만 빼기.
#[test]
fn marker_height_minimum_between_faces() {
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='240' w:lineRule='atLeast'/>",
        ["399", "798"],
        Some("approximate_space_before"),
    );
    check(
        28,
        "맑은 고딕",
        "<w:spacing w:line='320' w:lineRule='atLeast'/>",
        ["262", "524"],
        Some("approximate_space_before"),
    );
}
// 이것을 실패시키는 것: 강제 쪽 나눔에서 번호 높이 보정까지 제거.
#[test]
fn marker_height_survives_page_start() {
    check(
        28,
        "맑은 고딕",
        "<w:pageBreakBefore/><w:spacing w:before='120'/>",
        ["399", "798"],
        Some("approximate_space_before"),
    );
}
// 이것을 실패시키는 것: 두 자연 높이를 각각 반올림한 뒤 차이를 계산.
#[test]
fn marker_height_rounds_difference_once() {
    let out = convert_docx(&input(28, "맑은 고딕", "", &[21])).unwrap();
    let header = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    let section = xml::Element::parse(&package::section_xml(&out.bytes));
    let id = section.descendants("hp:p")[0].attr("paraPrIDRef").unwrap();
    let shape =
        header.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some(id)).unwrap();
    for (branch, value) in [("hp:case", "466"), ("hp:default", "932")] {
        assert_eq!(
            shape
                .child("hp:switch")
                .child(branch)
                .child("hh:margin")
                .child("hc:prev")
                .attr("value"),
            Some(value)
        );
    }
}
// 이것을 실패시키는 것: 수평 배치가 미지원이면 높이 보정도 조기 반환.
#[test]
fn marker_height_survives_unsupported_indent() {
    for (from, to) in
        [("w:hanging='360'", "w:firstLine='120'"), ("w:hanging='360'", "w:hanging='0'")]
    {
        let bytes =
            package::splice_part(&input(28, "맑은 고딕", "", &[22]), "numbering.xml", from, to);
        let out = convert_docx(&bytes).unwrap();
        let header = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
        let section = xml::Element::parse(&package::section_xml(&out.bytes));
        let id = section.descendants("hp:p")[0].attr("paraPrIDRef").unwrap();
        let shape =
            header.descendants("hh:paraPr").into_iter().find(|p| p.attr("id") == Some(id)).unwrap();
        assert_eq!(
            shape
                .child("hp:switch")
                .child("hp:case")
                .child("hh:margin")
                .child("hc:prev")
                .attr("value"),
            Some("399")
        );
    }
}

// 이것을 실패시키는 것: 합성 줄 대신 번호와 본문의 개별 전체 높이를 빼기.
#[test]
fn marker_height_combines_font_extrema() {
    // 맑은 고딕 ascent 2229, Calibri descent 550, UPM 2048:
    // 1100 * (550 - 495) / 2048 = 29.541015625 -> 30.
    let bytes = package::splice_part(
        &input(22, "맑은 고딕", "", &[22]),
        "numbering.xml",
        "맑은 고딕",
        "Calibri",
    );
    check_input(&bytes, ["30", "60"], Some("approximate_space_before"));
}
// 이것을 실패시키는 것: 번호가 모든 면에서 작은 경우에도 높이 차이를 더하기.
#[test]
fn marker_height_smaller_extrema_add_nothing() {
    // Calibri 9pt: ascent 856.93359375, descent 241.69921875.
    // 맑은 고딕 11pt: ascent 1197.216796875, descent 265.869140625.
    let bytes = package::splice_part(
        &input(18, "맑은 고딕", "", &[22]),
        "numbering.xml",
        "맑은 고딕",
        "Calibri",
    );
    check_input(&bytes, ["0", "0"], Some("retain_body_line_height"));
}
// 이것을 실패시키는 것: 셀 문단 최소 높이에서 번호 앞 간격 증가분 누락.
#[test]
fn marker_height_increases_cell_minimum() {
    for (marker, expected) in [(22, "1902"), (28, "2301")] {
        check_cell_minimum(&input(marker, "맑은 고딕", "", &[22]), "", "", expected);
    }
    // 자연 높이 1902에 단일 줄 번호 증가분 399를 한 번 더한 최소 높이 2301.
}

fn check_cell_minimum(bytes: &[u8], before: &str, after: &str, expected: &str) {
    let bytes = package::splice_part(
        bytes,
        "body.xml",
        "<w:body>",
        &format!("<w:body><w:tbl><w:tblPr><w:tblCellMar><w:top w:w='0'/><w:bottom w:w='0'/></w:tblCellMar></w:tblPr><w:tr><w:tc>{before}"),
    );
    let bytes = package::splice_part(
        &bytes,
        "body.xml",
        "</w:body>",
        &format!("{after}</w:tc></w:tr></w:tbl></w:body>"),
    );
    let out = convert_docx(&bytes).unwrap();
    let section = xml::Element::parse(&package::section_xml(&out.bytes));
    assert_eq!(section.find("hp:tc").child("hp:cellSz").attr("height"), Some(expected));
}

// 이것을 실패시키는 것: 번호의 첫 줄 증가분을 셀의 첫 문단에만 더하기.
#[test]
fn marker_height_counts_once_per_list_paragraph_in_cell() {
    for (marker, expected) in [(22, "3804"), (28, "4602")] {
        check_cell_minimum(&input(marker, "맑은 고딕", "", &[22, 22]), "", "", expected);
    }
}

// 이것을 실패시키는 것: 목록이 아닌 문단에도 번호의 첫 줄 증가분 더하기.
#[test]
fn marker_height_in_cell_follows_each_paragraph_kind() {
    let plain = "<w:p><w:pPr><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:hAnsi='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>본문 body</w:t></w:r></w:p>";
    let bytes = input(28, "맑은 고딕", "", &[22]);
    check_cell_minimum(&bytes, "", plain, "4203");
    check_cell_minimum(&bytes, plain, "", "4203");
}

// 이것을 실패시키는 것: 둘째 문단의 증가분을 문단 사이 간격과 큰 쪽으로 합치기.
#[test]
fn marker_height_in_cell_keeps_source_before_once() {
    check_cell_minimum(
        &input(28, "맑은 고딕", "<w:spacing w:before='120'/>", &[22, 22]),
        "",
        "",
        "5802",
    );
}

// 이것을 실패시키는 것: 미지원 배치의 번호 글자 모양 등록 또는 높이·스크립트 진단 생략.
#[test]
fn unsupported_marker_layout_measures_without_registering_shape() {
    for indent in [
        "<w:ind w:left='720' w:firstLine='120'/>",
        "<w:ind w:left='720' w:hanging='0'/><w:tabs><w:tab w:val='num' w:pos='720'/></w:tabs>",
    ] {
        let bytes = package::splice_part(
            &input(28, "맑은 고딕", "", &[22, 22]),
            "numbering.xml",
            "<w:ind w:left='720' w:hanging='360'/>",
            indent,
        );
        let bytes = package::splice_part(
            &bytes,
            "numbering.xml",
            "<w:sz w:val='28'/>",
            "<w:sz w:val='28'/><w:szCs w:val='22'/>",
        );
        let out = convert_docx(&bytes).unwrap();
        let header = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
        let section = xml::Element::parse(&package::section_xml(&out.bytes));
        assert_eq!(header.descendants("hh:charPr").len(), 8);
        assert_eq!(
            header
                .descendants("hh:charPr")
                .iter()
                .filter(|c| c.attr("height") == Some("1400"))
                .count(),
            0
        );
        for p in section.descendants("hp:p") {
            let shape = header
                .descendants("hh:paraPr")
                .into_iter()
                .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            assert_eq!(
                shape
                    .child("hp:switch")
                    .child("hp:case")
                    .child("hh:margin")
                    .child("hc:prev")
                    .attr("value"),
                Some("399")
            );
        }
        for loss in ["script_format", "list_marker_line_height"] {
            let warnings: Vec<_> = out
                .warnings
                .iter()
                .filter_map(|w| match w {
                    OpsWarning::Docx(w) if w.loss() == loss => Some(w.count()),
                    _ => None,
                })
                .collect();
            assert_eq!(warnings, [1], "{loss}");
        }
    }
}
