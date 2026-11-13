#![cfg(feature = "ops-docx")]
use hwpforge::{
    hwpx::{EncodeOutcome, EncodeWarning},
    ops::{convert_docx, take_bytes_fail_closed},
};
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;
fn numbered(extra: &str, direct: &str) -> Vec<u8> {
    let defs=format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:pPr><w:ind w:left='720' w:hanging='360'/></w:pPr><w:rPr><w:sz w:val='22'/></w:rPr>{extra}</w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
    let item = |p: &str| {
        format!("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr>{p}</w:pPr><w:r><w:t>body</w:t></w:r></w:p>")
    };
    docx::package_with_numbering(&format!("{}{}", item(""), item(direct)), &defs)
}
// 이것을 실패시키는 것: import에서 DOCX 근사 또는 HWPX 의미 손실 경고 폐기.
#[test]
fn imported_list_layout_keeps_projection_diagnostics() {
    let result =
        convert_docx(&numbered("<w:suff w:val='space'/><w:lvlJc w:val='right'/>", "")).unwrap();
    assert!(result.warnings.iter().any(
        |w| matches!(w,hwpforge::ops::OpsWarning::Docx(w) if w.loss()=="list_marker_space_width")
    ));
    assert!(result.warnings.iter().any(|w|matches!(w,hwpforge::ops::OpsWarning::Encode(EncodeWarning::ContentLost{kind,..}) if kind=="list_marker_alignment")));
    let header = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    let n =
        header.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("2")).unwrap();
    let head =
        n.descendants("hh:paraHead").into_iter().find(|h| h.attr("level") == Some("1")).unwrap();
    assert_eq!(head.attr("textOffsetType"), Some("HWPUNIT"));
    assert_eq!(head.attr("textOffset"), Some("306"));
}
// 이것을 실패시키는 것: 편집 실패 관문에서 새 ContentLost kind를 무시.
#[test]
fn marker_loss_fails_closed_in_editors() {
    let ordinary = numbered("", "");
    let roman =
        package::splice_part(&ordinary, "numbering.xml", "w:val='decimal'", "w:val='upperRoman'");
    for (input, kind) in [
        (numbered("<w:suff w:val='space'/><w:lvlJc w:val='right'/>", ""), "list_marker_alignment"),
        (numbered("", "<w:ind w:left='960' w:hanging='480'/>"), "list_marker_conflict"),
        (roman, "list_marker_base_width"),
    ] {
        let result = convert_docx(&input).unwrap();
        let warnings: Vec<_> = result
            .warnings
            .into_iter()
            .filter_map(|w| match w {
                hwpforge::ops::OpsWarning::Encode(w) => Some(w),
                _ => None,
            })
            .collect();
        assert!(warnings
            .iter()
            .any(|w| matches!(w,EncodeWarning::ContentLost{kind:k,..} if k==kind)));
        assert!(take_bytes_fail_closed(EncodeOutcome { bytes: result.bytes, warnings }).is_err());
    }
    let bytes = convert_docx(&ordinary).unwrap().bytes;
    let mut decoded = hwpforge::hwpx::HwpxDecoder::decode(&bytes).unwrap();
    let id = decoded.document.sections()[0].paragraphs[0].para_shape_id.get();
    decoded.document.sections_mut()[0].paragraphs[0]
        .add_run(hwpforge::core::Run::text("mixed", hwpforge::foundation::CharShapeIndex::new(0)));
    decoded
        .style_store
        .set_paragraph_head_metrics(
            hwpforge::foundation::HeadingType::Number,
            2,
            1,
            hwpforge::foundation::Alignment::Left,
            false,
            true,
            700,
            0,
            false,
            u32::MAX,
        )
        .unwrap();
    let mut value = serde_json::to_value(&decoded.style_store).unwrap();
    value["list_marker_layouts"] = serde_json::json!({});
    value["list_marker_layouts"][id.to_string()] =
        serde_json::json!({"align":"left","text_start":{"kind":"from_anchor","value":1800}});
    let store = serde_json::from_value(value).unwrap();
    let out = hwpforge::hwpx::HwpxEncoder::encode_with_diagnostics(
        &decoded.document.validate().unwrap(),
        &store,
        &decoded.image_store,
        hwpforge::hwpx::EncodeOptions::default(),
    )
    .unwrap();
    assert!(out.warnings.iter().any(
        |w| matches!(w,EncodeWarning::ContentLost{kind,..} if kind=="list_marker_font_context")
    ));
    assert!(take_bytes_fail_closed(out).is_err());
}

// 이것을 실패시키는 것: 0칸의 기존 paraHead를 고정 폭으로 교체하거나 경고 중복.
#[test]
fn imported_unsupported_zero_hanging_retains_legacy_head_metrics() {
    let input = numbered("<w:suff w:val='tab'/>", "");
    let input = package::splice_part(&input, "numbering.xml", "w:hanging='360'", "w:hanging='0'");
    let input =
        package::splice_part(&input, "numbering.xml", "w:val='decimal'", "w:val='upperRoman'");
    let result = convert_docx(&input).unwrap();
    let warnings: Vec<_> = result
        .warnings
        .iter()
        .filter_map(|w| match w {
            hwpforge::ops::OpsWarning::Docx(w) if w.loss() == "list_marker_zero_slot" => {
                Some(w.count())
            }
            _ => None,
        })
        .collect();
    assert_eq!(warnings, [1]);
    let h = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    let n = h.descendants("hh:numbering").into_iter().find(|n| n.attr("id") == Some("2")).unwrap();
    let head =
        n.descendants("hh:paraHead").into_iter().find(|h| h.attr("level") == Some("1")).unwrap();
    for (name, expected) in [
        ("align", "LEFT"),
        ("useInstWidth", "1"),
        ("autoIndent", "1"),
        ("widthAdjust", "0"),
        ("textOffsetType", "PERCENT"),
        ("textOffset", "50"),
        ("charPrIDRef", "4294967295"),
    ] {
        assert_eq!(head.attr(name), Some(expected), "{name}");
    }
}

// 이것을 실패시키는 것: 디코더가 새 배치와 함께 접지 않은 first-line indent를 반환.
#[test]
fn decoded_marker_registry_accepts_folded_indentation() {
    let d = hwpforge::docx::decode_docx(&numbered("", "<w:ind w:left='960' w:hanging='480'/>"))
        .unwrap();
    let restored: hwpforge::blueprint::registry::StyleRegistry =
        serde_json::from_value(serde_json::to_value(&d.registry).unwrap()).unwrap();
    let requests: Vec<_> =
        restored.para_shapes.iter().filter(|p| p.list_marker.is_some()).collect();
    assert!(!requests.is_empty());
    assert!(requests.iter().all(|p| p.indent_first_line.as_i32() == 0));
}

fn imported_head(h: &xml::Element, bullet: bool) -> &xml::Element {
    let (tag, id, level) =
        if bullet { ("hh:bullet", "1", "0") } else { ("hh:numbering", "2", "1") };
    h.descendants(tag)
        .into_iter()
        .find(|n| n.attr("id") == Some(id))
        .unwrap()
        .descendants("hh:paraHead")
        .into_iter()
        .find(|h| h.attr("level") == Some(level))
        .unwrap()
}
fn imported_metrics(h: &xml::Element, bullet: bool, expected: [&str; 6]) {
    let head = imported_head(h, bullet);
    for (name, value) in
        ["align", "useInstWidth", "autoIndent", "widthAdjust", "textOffsetType", "textOffset"]
            .into_iter()
            .zip(expected)
    {
        assert_eq!(head.attr(name), Some(value), "{name}");
    }
    let character = head.attr("charPrIDRef").unwrap();
    let shape =
        h.descendants("hh:charPr").into_iter().find(|c| c.attr("id") == Some(character)).unwrap();
    assert_eq!(shape.attr("height"), Some("1100"));
}
// 이것을 실패시키는 것: DOCX 기본 탭 요청/번호 문자 참조 전달 누락 또는 HWPX margin 이동.
#[test]
fn imported_zero_hanging_uses_default_tab_grid() {
    for (tab, widths) in [
        ("720", ["2500", "0", "3000", "2500", "2500", "2500"]),
        ("800", ["3300", "400", "-200", "2900", "3300", "3300"]),
    ] {
        for (i, anchor) in [720, 500, 620, 0, 720, 720].into_iter().enumerate() {
            let bullet = i == 5;
            let format = if bullet { "bullet" } else { "decimal" };
            let text = if bullet { "&#xF0B7;" } else { "%1." };
            let fonts = if bullet { "<w:rFonts w:ascii='Symbol' w:hAnsi='Symbol'/>" } else { "" };
            let start = if i == 4 { 9 } else { 1 };
            let defs=format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='{start}'/><w:numFmt w:val='{format}'/><w:lvlText w:val='{text}'/><w:pPr><w:ind w:left='{anchor}' w:hanging='0'/></w:pPr><w:rPr><w:sz w:val='22'/>{fonts}</w:rPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
            let body="<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr><w:r><w:rPr><w:sz w:val='22'/></w:rPr><w:t>body</w:t></w:r></w:p>".repeat(2);
            let input = docx::package_with_numbering_and_settings(&body, &defs, Some(tab));
            let result = convert_docx(&input).unwrap();
            let h = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
            imported_metrics(&h, bullet, ["LEFT", "0", "1", widths[i], "PERCENT", "0"]);
            let section = xml::Element::parse(&package::section_xml(&result.bytes));
            let paragraphs = section.descendants("hp:p");
            let reference = paragraphs[0].attr("paraPrIDRef").unwrap();
            assert_eq!(paragraphs[1].attr("paraPrIDRef"), Some(reference));
            let shape = h
                .descendants("hh:paraPr")
                .into_iter()
                .find(|p| p.attr("id") == Some(reference))
                .unwrap();
            let heading = shape.child("hh:heading");
            assert_eq!(heading.attr("idRef"), Some(if bullet { "1" } else { "2" }));
            let switch = shape.child("hp:switch");
            for (branch, values) in [
                ("hp:case", ["3600", "2500", "3100", "0", "3600", "3600"]),
                ("hp:default", ["7200", "5000", "6200", "0", "7200", "7200"]),
            ] {
                let margin = switch.child(branch).child("hh:margin");
                assert_eq!(margin.child("hc:left").attr("value"), Some(values[i]));
                assert_eq!(margin.child("hc:intent").attr("value"), Some("0"));
            }
            let marker_ref = imported_head(&h, bullet).attr("charPrIDRef").unwrap();
            let body_ref = paragraphs[0]
                .descendants("hp:run")
                .into_iter()
                .find(|r| !r.descendants("hp:t").is_empty())
                .unwrap()
                .attr("charPrIDRef")
                .unwrap();
            assert_eq!(marker_ref, body_ref);
            for (loss, want) in [
                ("list_marker_default_tab", 1),
                ("list_marker_continuation", 1),
                ("list_marker_zero_slot", 0),
                ("list_marker_tab_boundary", u64::from(tab == "800" && anchor == 620)),
            ] {
                assert_eq!(
                    result
                        .warnings
                        .iter()
                        .filter_map(|w| match w {
                            hwpforge::ops::OpsWarning::Docx(w) if w.loss() == loss =>
                                Some(w.count()),
                            _ => None,
                        })
                        .sum::<u64>(),
                    want,
                    "{loss}"
                );
            }
        }
    }
}
// 이것을 실패시키는 것: max 제거 → 두 기본 탭 간격 모두 Right widthAdjust −1100 · margin 3600/7200,
// 오른쪽 anchor 후보 제거 또는 1400 번호 폭 대신 본문 크기 사용.
#[test]
fn imported_zero_hanging_alignment_and_marker_size() {
    for (align, size, anchor, tab, width, offset, left, default) in [
        ("right", 22, 720, "720", "0", "0", "2500", "5000"),
        ("right", 22, 720, "800", "0", "0", "2500", "5000"),
        ("center", 22, 720, "720", "6100", "0", "0", "0"),
        ("center", 22, 720, "800", "7700", "0", "-800", "-1600"),
        ("left", 28, 600, "720", "2800", "0", "3000", "6000"),
    ] {
        let defs=format!("<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:lvlJc w:val='{align}'/><w:pPr><w:ind w:left='{anchor}' w:firstLine='0'/></w:pPr><w:rPr><w:sz w:val='{size}'/></w:rPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/></w:num>");
        let input=docx::package_with_numbering_and_settings("<w:p><w:pPr><w:numPr><w:numId w:val='7'/></w:numPr></w:pPr><w:r><w:rPr><w:sz w:val='22'/></w:rPr><w:t>body</w:t></w:r></w:p>",&defs,Some(tab));
        let out = convert_docx(&input).unwrap();
        let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
        let head = imported_head(&h, false);
        for (k, v) in [
            ("align", align.to_uppercase()),
            ("useInstWidth", "0".into()),
            ("autoIndent", "1".into()),
            ("widthAdjust", width.into()),
            ("textOffsetType", if align == "left" { "PERCENT" } else { "HWPUNIT" }.into()),
            ("textOffset", offset.into()),
        ] {
            assert_eq!(head.attr(k), Some(v.as_str()));
        }
        let c = h
            .descendants("hh:charPr")
            .into_iter()
            .find(|c| c.attr("id") == head.attr("charPrIDRef"))
            .unwrap();
        assert_eq!(c.attr("height"), Some(if size == 28 { "1400" } else { "1100" }));
        let section = xml::Element::parse(&package::section_xml(&out.bytes));
        let p = section.descendants("hp:p")[0];
        let shape = h
            .descendants("hh:paraPr")
            .into_iter()
            .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
            .unwrap();
        for (branch, value) in [("hp:case", left), ("hp:default", default)] {
            assert_eq!(
                shape
                    .child("hp:switch")
                    .child(branch)
                    .child("hh:margin")
                    .child("hc:left")
                    .attr("value"),
                Some(value)
            );
        }
    }
}

// 이것을 실패시키는 것: 충돌 경고를 버리거나 unchanged edit-base의 조기 반환에서 누락.
#[test]
fn marker_conflict_fails_closed_with_selected_wire() {
    let result = convert_docx(&numbered("", "<w:ind w:left='960' w:hanging='480'/>")).unwrap();
    let h = xml::Element::parse(&package::part(&result.bytes, "Contents/header.xml"));
    imported_metrics(&h, false, ["LEFT", "0", "1", "700", "PERCENT", "0"]);
    let base = hwpforge::hwpx::decoder::HwpxEditBase::read(&result.bytes).unwrap();
    let decoded = base.decoded();
    let mut value = serde_json::to_value(&decoded.style_store).unwrap();
    let first = decoded.document.sections()[0].paragraphs[0].para_shape_id.get().to_string();
    let second = decoded.document.sections()[0].paragraphs[1].para_shape_id.get().to_string();
    let character = imported_head(&h, false).attr("charPrIDRef").unwrap().parse::<u32>().unwrap();
    value["list_marker_layouts"] = serde_json::json!({});
    for (id, gap) in [(first, 1800), (second, 2400)] {
        value["list_marker_layouts"][id] = serde_json::json!({"align":"left","text_start":{"kind":"from_anchor","value":gap},"char_shape":character});
    }
    let store = serde_json::from_value(value).unwrap();
    let document = decoded.document.clone().validate().unwrap();
    let out = hwpforge::hwpx::HwpxEncoder::encode_with_edit_base(
        &base,
        &document,
        &store,
        &decoded.image_store,
        hwpforge::hwpx::EncodeOptions::default(),
    )
    .unwrap();
    assert_eq!(out.bytes, result.bytes);
    assert_eq!(
        out.warnings
            .iter()
            .filter(
                |w| matches!(w,EncodeWarning::ContentLost{kind,..} if kind=="list_marker_conflict")
            )
            .count(),
        1
    );
    imported_metrics(
        &xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml")),
        false,
        ["LEFT", "0", "1", "700", "PERCENT", "0"],
    );
    assert!(take_bytes_fail_closed(out).is_err());
}

fn styled_numbering(direct: bool, style_first: &str, direct_indent: &str) -> Vec<u8> {
    use std::io::{Cursor, Read, Write};
    let body = [0, 1].map(|level| {
        let num = if direct { format!("<w:numPr><w:ilvl w:val='{level}'/><w:numId w:val='7'/></w:numPr>") } else { String::new() };
        format!("<w:p><w:pPr><w:pStyle w:val='list{level}'/>{num}{direct_indent}</w:pPr><w:r><w:rPr><w:sz w:val='22'/></w:rPr><w:t>body</w:t></w:r></w:p>")
    }).join("");
    let defs = "<w:abstractNum w:abstractNumId='1'><w:lvl w:ilvl='0'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%1.'/><w:pPr><w:ind w:left='440' w:right='120' w:hanging='440'/><w:spacing w:before='120'/><w:jc w:val='right'/></w:pPr></w:lvl><w:lvl w:ilvl='1'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%2.'/><w:pPr><w:ind w:left='880' w:right='120' w:hanging='440'/></w:pPr></w:lvl></w:abstractNum><w:num w:numId='7'><w:abstractNumId w:val='1'/><w:lvlOverride w:ilvl='1'><w:lvl w:ilvl='1'><w:start w:val='1'/><w:numFmt w:val='decimal'/><w:lvlText w:val='%2.'/><w:lvlJc w:val='left'/><w:pPr><w:ind w:left='880' w:right='160' w:hanging='440'/></w:pPr></w:lvl></w:lvlOverride></w:num>";
    let input = docx::package_with_numbering(&body, defs);
    let input = package::splice_part(&input, "[Content_Types].xml", "</Types>", "<Override PartName='/styles.xml' ContentType='application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml'/></Types>");
    let input = package::splice_part(&input, "_rels/body.xml.rels", "</Relationships>", "<Relationship Id='styles' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles' Target='styles.xml'/></Relationships>");
    let mut source = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut file = source.by_index(i).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        output.start_file(file.name().unwrap(), zip::write::SimpleFileOptions::default()).unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.start_file("styles.xml", zip::write::SimpleFileOptions::default()).unwrap();
    let styles = [0, 1].map(|level| {
        let num = if direct { String::new() } else { format!("<w:numPr><w:ilvl w:val='{level}'/><w:numId w:val='7'/></w:numPr>") };
        format!("<w:style w:type='paragraph' w:styleId='list{level}'><w:pPr>{num}<w:ind w:left='720' w:right='360' {style_first}/><w:spacing w:before='240'/><w:jc w:val='center'/></w:pPr></w:style>")
    }).join("");
    write!(output, "<w:styles xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'>{styles}</w:styles>").unwrap();
    output.finish().unwrap().into_inner()
}

fn assert_styled_numbering(
    input: &[u8],
    anchors: [i32; 2],
    right: [i32; 2],
    xml_left: [&str; 2],
    xml_default: [&str; 2],
) {
    let decoded = hwpforge::docx::decode_docx(input).unwrap();
    for (i, p) in decoded.document.sections()[0].paragraphs.iter().enumerate() {
        let shape = &decoded.registry.para_shapes[p.para_shape_id.get()];
        assert_eq!(shape.indent_left.as_i32(), anchors[i]);
        assert_eq!(shape.indent_right.as_i32(), right[i]);
        assert_eq!(shape.indent_first_line.as_i32(), 0);
        assert_eq!(
            shape.list_marker.unwrap().text_start,
            hwpforge::blueprint::ListTextStart::FromAnchor(
                hwpforge::foundation::HwpUnit::new(2200).unwrap()
            )
        );
    }
    let out = convert_docx(input).unwrap();
    let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    let section = xml::Element::parse(&package::section_xml(&out.bytes));
    for (i, p) in section.descendants("hp:p").into_iter().enumerate() {
        let shape = h
            .descendants("hh:paraPr")
            .into_iter()
            .find(|s| s.attr("id") == p.attr("paraPrIDRef"))
            .unwrap();
        assert_eq!(shape.child("hh:align").attr("horizontal"), Some("CENTER"));
        let switch = shape.child("hp:switch");
        for (branch, left) in [("hp:case", xml_left[i]), ("hp:default", xml_default[i])] {
            let margin = switch.child(branch).child("hh:margin");
            assert_eq!(margin.child("hc:left").attr("value"), Some(left));
            assert_eq!(margin.child("hc:intent").attr("value"), Some("0"));
            assert_eq!(
                margin.child("hc:prev").attr("value"),
                Some(if i == 1 {
                    "0"
                } else if branch == "hp:case" {
                    "1200"
                } else {
                    "2400"
                })
            );
        }
        let heading = shape.child("hh:heading");
        let numbering = h
            .descendants("hh:numbering")
            .into_iter()
            .find(|n| n.attr("id") == heading.attr("idRef"))
            .unwrap();
        let level = if i == 0 { "1" } else { "2" };
        let head = numbering
            .descendants("hh:paraHead")
            .into_iter()
            .find(|h| h.attr("level") == Some(level))
            .unwrap();
        for (name, value) in [
            ("align", "LEFT"),
            ("useInstWidth", "0"),
            ("autoIndent", "1"),
            ("widthAdjust", "1200"),
            ("textOffsetType", "PERCENT"),
            ("textOffset", "0"),
        ] {
            assert_eq!(head.attr(name), Some(value), "{name}");
        }
    }
}

// 이것을 실패시키는 것: 문단 스타일 left/right를 번호 수준·override 뒤에 덮기 또는 들여쓰기 밖 정렬·간격도 재적용.
#[test]
fn direct_numbering_indent_precedes_paragraph_style() {
    assert_styled_numbering(
        &styled_numbering(true, "", ""),
        [0, 2200],
        [600, 800],
        ["0", "2200"],
        ["0", "4400"],
    );
}

// 이것을 실패시키는 것: 번호 들여쓰기를 직접 pPr 뒤에 적용.
#[test]
fn direct_numbering_indent_keeps_direct_paragraph_priority() {
    assert_styled_numbering(
        &styled_numbering(true, "", "<w:ind w:left='1000'/>"),
        [2800, 2800],
        [600, 800],
        ["2800", "2800"],
        ["5600", "5600"],
    );
}

// 이것을 실패시키는 것: 스타일 상속 numPr에도 번호 들여쓰기 우선순위 적용.
#[test]
fn inherited_numbering_indent_keeps_paragraph_style_priority() {
    assert_styled_numbering(
        &styled_numbering(false, "", ""),
        [1400, 1400],
        [1800, 1800],
        ["1400", "1400"],
        ["2800", "2800"],
    );
}

// 이것을 실패시키는 것: first 슬롯 재적용 누락으로 스타일 firstLine과 번호 hanging 혼합.
#[test]
fn direct_numbering_hanging_replaces_style_first_line_slot() {
    assert_styled_numbering(
        &styled_numbering(true, "w:firstLine='200'", ""),
        [0, 2200],
        [600, 800],
        ["0", "2200"],
        ["0", "4400"],
    );
}
