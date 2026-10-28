#[path = "support/document.rs"]
mod document;
#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

use hwpforge_core::{
    chart::{ChartData, ChartType},
    control::{PathFieldCommand, RefTarget},
    Control, Document, PageSettings, RunContent, Section,
};
use hwpforge_foundation::{HwpUnit, RefContentType, RefType};
use hwpforge_smithy_hwpx::HwpxDecoder;
use xml::Element;

fn encode(control: Control) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![document::runs_para(vec![document::control_run(control)])],
        PageSettings::a4(),
    ));
    encoding::encode(doc)
}

fn section(control: Control) -> Element {
    Element::parse(&package::section_xml(&encode(control)))
}

fn chart(scatter: bool) -> Control {
    if scatter {
        Control::chart(ChartType::Scatter, ChartData::xy(&[("Sales", &[1., 2.], &[3., 4.])]))
    } else {
        Control::chart(ChartType::Column, ChartData::category(&["A", "B"], &[("Sales", &[3., 4.])]))
    }
}

/// The parsed text keeps character and entity references and CDATA,
/// which quick-xml reports apart from the text around them.
/// 이것을 실패시키는 것: `Element::parse` 의 `Event::GeneralRef` 또는 `Event::CData` 처리 삭제.
#[test]
fn xml_helper_keeps_references_in_text() {
    assert_eq!(Element::parse("<a>A &amp; B&#xAC00;</a>").text, "A & B가");
    assert_eq!(Element::parse("<a>Sales<![CDATA[ <x>]]></a>").text, "Sales <x>");
}

/// 이것을 실패시키는 것: <c:f>Sheet1!$A$2:$A${end_row}</c:f>,
/// <c:f>Sheet1!${val_col}$2:${val_col}${end_row}</c:f>,
/// <c:f>Sheet1!${x_col}$2:${x_col}${end_row}</c:f>,
/// <c:f>Sheet1!${y_col}$2:${y_col}${end_row}</c:f> 삭제.
#[test]
fn chart_references_keep_formula_before_cache() {
    for scatter in [false, true] {
        let bytes = encode(chart(scatter));
        let section = Element::parse(&package::section_xml(&bytes));
        let part = section.find("hp:chart").attr("chartIDRef").unwrap();
        let xml = Element::parse(&package::part(&bytes, part));
        let refs = if scatter {
            [
                ("c:xVal", "c:numRef", "c:numCache", "Sheet1!$A$2:$A$3"),
                ("c:yVal", "c:numRef", "c:numCache", "Sheet1!$B$2:$B$3"),
            ]
        } else {
            [
                ("c:cat", "c:strRef", "c:strCache", "Sheet1!$A$2:$A$3"),
                ("c:val", "c:numRef", "c:numCache", "Sheet1!$B$2:$B$3"),
            ]
        };
        for (parent, reference, cache, formula) in refs {
            let node = xml.find(parent).child(reference);
            assert_eq!(node.child_names(), ["c:f", cache]);
            assert_eq!(node.child("c:f").text, formula);
        }
    }
}

/// 이것을 실패시키는 것: <c:tx><c:v>{}</c:v></c:tx> → <c:tx><c:f>{}</c:f></c:tx>.
#[test]
fn series_names_are_literal_values() {
    for scatter in [false, true] {
        let bytes = encode(chart(scatter));
        let section = Element::parse(&package::section_xml(&bytes));
        let xml = Element::parse(&package::part(
            &bytes,
            section.find("hp:chart").attr("chartIDRef").unwrap(),
        ));
        let tx = xml.find("c:ser").child("c:tx");
        assert_eq!(tx.child_names(), ["c:v"]);
        assert_eq!(tx.child("c:v").text, "Sales");
    }
}

/// 이것을 실패시키는 것: DropCapStyle::None.to_string() → String::new() 또는 DropCapStyle::DoubleLine.to_string().
#[test]
fn structured_chart_dropcap_is_none() {
    assert_eq!(section(chart(false)).find("hp:chart").attr("dropcapstyle"), Some("None"));
}

/// 이것을 실패시키는 것: dropcapstyle="None" → 삭제, "DoubleLine", "NONE" (chart 및 ole).
#[test]
fn embedded_chart_and_ole_dropcap_are_none() {
    let xml = section(Control::EmbeddedChart {
        chart_xml:
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"/>"#
                .into(),
        ole_bytes: vec![0; 16],
        width: HwpUnit::new(20000).unwrap(),
        height: HwpUnit::new(10000).unwrap(),
        placement: None,
    });
    let switch = xml.find("hp:switch");
    assert_eq!(switch.child("hp:case").child("hp:chart").attr("dropcapstyle"), Some("None"));
    assert_eq!(switch.child("hp:default").child("hp:ole").attr("dropcapstyle"), Some("None"));
}

/// 이것을 실패시키는 것: 글맵시 dropcapstyle="None" → dropcapstyle="NONE".
#[test]
fn textart_dropcap_is_none() {
    let xml = section(Control::TextArt {
        frame: hwpforge_core::ObjectFrame::new(),
        text: "글".into(),
        shape: "WAVE2".into(),
        font_name: "함초롬바탕".into(),
        font_style: "보통".into(),
        align: "LEFT".into(),
        line_spacing: 120,
        char_spacing: 100,
        width: HwpUnit::new(14000).unwrap(),
        height: HwpUnit::new(2400).unwrap(),
        placement: None,
        fill_color: None,
        inst_id: None,
    });
    assert_eq!(xml.find("hp:textart").attr("dropcapstyle"), Some("None"));
}

/// 이것을 실패시키는 것: left: 56 또는 right: 56 → 55 또는 57.
#[test]
fn equation_margins_are_exact() {
    let xml = section(Control::equation("a+b"));
    let margin = xml.find("hp:equation").child("hp:outMargin");
    for (name, value) in [("left", "56"), ("right", "56"), ("top", "0"), ("bottom", "0")] {
        assert_eq!(margin.attr(name), Some(value), "{name}");
    }
}

/// 이것을 실패시키는 것: fieldBegin 또는 fieldEnd 의
/// 627272811 → 628321650, 628321650 → 628121972, 628121972 → 627272811.
#[test]
fn field_type_ids_match_on_both_boundaries() {
    for (control, kind, id) in [
        (Control::field("입력"), "CLICK_HERE", "627272811"),
        (
            Control::DateCodeField { is_time_mode: false, display_text: "2026".into() },
            "SUMMERY",
            "628321650",
        ),
        (
            Control::PathField {
                command: PathFieldCommand::PathAndFileName,
                display_text: "a.hwpx".into(),
            },
            "PATH",
            "628121972",
        ),
    ] {
        let xml = section(control);
        let begin = xml.find("hp:fieldBegin");
        let end = xml.find("hp:fieldEnd");
        assert_eq!(begin.attr("type"), Some(kind));
        assert_eq!(begin.attr("fieldid"), Some(id), "begin {kind}");
        assert_eq!(end.attr("fieldid"), Some(id), "end {kind}");
        assert_eq!(end.attr("beginIDRef"), begin.attr("id"));
    }
}

fn crossref() -> Control {
    Control::cross_ref(
        RefTarget::Name("target".into()),
        RefType::Bookmark,
        RefContentType::Contents,
    )
}

/// 이것을 실패시키는 것: Prop 삭제, Fiexde/Prop 순서 교환,
/// Prop → Props, Command → Commands, Fiexde → Fixed.
#[test]
fn crossref_parameters_keep_wire_names_and_order() {
    let xml = section(crossref());
    let params = xml.find("hp:fieldBegin").child("hp:parameters");
    assert_eq!(params.attr("cnt"), Some("8"));
    assert_eq!(
        params.child_names(),
        [
            "hp:booleanParam",
            "hp:integerParam",
            "hp:stringParam",
            "hp:stringParam",
            "hp:stringParam",
            "hp:stringParam",
            "hp:booleanParam",
            "hp:stringParam"
        ]
    );
    assert_eq!(
        params.children.iter().map(|c| c.attr("name").unwrap()).collect::<Vec<_>>(),
        [
            "Fiexde",
            "Prop",
            "Command",
            "RefPath",
            "RefType",
            "RefContentType",
            "RefHyperLink",
            "RefOpenType"
        ]
    );
}

/// 이것을 실패시키는 것: "OBJECT_TYPE_CONTENTS" | "Contents" | "contents" => Ok(Self::Contents) → Ok(Self::UpDownPos).
#[test]
fn crossref_contents_survives_wire_roundtrip() {
    let bytes = encode(crossref());
    let xml = Element::parse(&package::section_xml(&bytes));
    let params = xml.find("hp:parameters");
    assert_eq!(
        params.children.iter().find(|c| c.attr("name") == Some("RefContentType")).unwrap().text,
        "OBJECT_TYPE_CONTENTS"
    );
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let content_type = decoded.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .find_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::CrossRef { content_type, .. } => Some(content_type),
                _ => None,
            },
            _ => None,
        })
        .expect("cross reference");
    assert_eq!(*content_type, RefContentType::Contents);
}
