#![cfg(feature = "ops-docx")]
#[path = "support/docx.rs"]
mod docx;
#[path = "../../hwpforge-smithy-hwpx/tests/support/package.rs"]
mod package;
#[path = "../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;

const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

fn convert(settings: Option<&str>) -> hwpforge::ops::ImportOutput {
    let body = "<w:p/><w:p><w:pPr><w:rPr><w:sz w:val='26'/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val='22'/></w:rPr><w:t>A B 가나다 123</w:t></w:r><w:r><w:rPr><w:b/><w:sz w:val='32'/></w:rPr><w:t>C D</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>E F</w:t></w:r></w:p></w:tc></w:tr></w:tbl>";
    let mut input = docx::package_with_numbering_and_settings(body, "", settings.map(|_| "720"));
    if let Some(settings) = settings {
        input = package::splice_part(
            &input,
            "options.xml",
            &format!("<w:settings xmlns:w='{W}'><w:defaultTabStop w:val='720'/></w:settings>"),
            settings,
        );
    }
    hwpforge::ops::convert_docx(&input).unwrap()
}

fn assert_used_space(settings: Option<&str>, expected: &str) {
    let out = convert(settings);
    let header = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    let section = xml::Element::parse(&package::section_xml(&out.bytes));
    let runs = section.descendants("hp:run");
    assert!(runs.len() >= 5);
    for run in runs {
        let reference = run.attr("charPrIDRef").unwrap();
        let shape = header
            .descendants("hh:charPr")
            .into_iter()
            .find(|shape| shape.attr("id") == Some(reference))
            .unwrap();
        assert_eq!(shape.attr("useFontSpace"), Some(expected), "{settings:?}: {reference}");
    }
}

// 이것을 실패시키는 것: 설정 전달 누락, 존재를 꺼짐으로 해석, 일부 사용 글자 모양만 변경.
#[test]
fn balanced_space_width_enabled() {
    for value in ["", " w:val='1'", " w:val='true'", " w:val='on'"] {
        assert_used_space(
            Some(&format!("<w:settings xmlns:w='{W}'><w:compat><w:balanceSingleByteDoubleByteWidth{value}/></w:compat></w:settings>")),
            "0",
        );
    }
}

// 이것을 실패시키는 것: 설정이 없는 문서에도 반각 공백을 기본 적용.
#[test]
fn balanced_space_width_absent() {
    assert_used_space(None, "1");
    assert_used_space(Some(&format!("<w:settings xmlns:w='{W}'><w:compat/></w:settings>")), "1");
}

// 이것을 실패시키는 것: 요소 존재만 보고 w:val='0'/false/off를 켬으로 처리.
#[test]
fn balanced_space_width_disabled() {
    for value in ["0", "false", "off"] {
        assert_used_space(
            Some(&format!("<w:settings xmlns:w='{W}'><w:compat><w:balanceSingleByteDoubleByteWidth w:val='{value}'/></w:compat></w:settings>")),
            "1",
        );
    }
}

// 이것을 실패시키는 것: 다른 namespace·compat 밖·중첩·잘못된 루트의 동명 요소를 채택.
#[test]
fn balanced_space_width_scoped() {
    for settings in [
        format!("<w:settings xmlns:w='{W}' xmlns:x='urn:other'><w:compat><x:balanceSingleByteDoubleByteWidth/></w:compat></w:settings>"),
        format!("<w:settings xmlns:w='{W}' xmlns:x='urn:other'><x:compat><w:balanceSingleByteDoubleByteWidth/></x:compat></w:settings>"),
        format!("<w:settings xmlns:w='{W}'><w:balanceSingleByteDoubleByteWidth/></w:settings>"),
        format!("<w:settings xmlns:w='{W}'><w:compat><w:wrapper><w:balanceSingleByteDoubleByteWidth/></w:wrapper></w:compat></w:settings>"),
        format!("<w:settings xmlns:w='{W}'><w:wrapper><w:compat><w:balanceSingleByteDoubleByteWidth/></w:compat></w:wrapper></w:settings>"),
        format!("<w:wrong xmlns:w='{W}'><w:compat><w:balanceSingleByteDoubleByteWidth/></w:compat></w:wrong>"),
        format!("<x:settings xmlns:w='{W}' xmlns:x='urn:other'><w:compat><w:balanceSingleByteDoubleByteWidth/></w:compat></x:settings>"),
    ] {
        assert_used_space(Some(&settings), "1");
    }
}

// 이것을 실패시키는 것: 처리한 요소의 무시 진단 유지, 다른 compat 진단 제거, 잘못된 boolean 묵인.
#[test]
fn balanced_space_width_diagnostics() {
    for (children, expected) in [
        ("<w:balanceSingleByteDoubleByteWidth/>", vec![]),
        ("<w:balanceSingleByteDoubleByteWidth w:val='0'/>", vec![]),
        ("<w:balanceSingleByteDoubleByteWidth/><w:compatSetting w:name='x'/>", vec![("DOCX_DOCUMENT_SETTINGS_OMITTED", "document_settings", "ignore", 1)]),
        ("<w:compatSetting w:name='x'/><w:balanceSingleByteDoubleByteWidth/><w:usePrinterMetrics/>", vec![("DOCX_DOCUMENT_SETTINGS_OMITTED", "document_settings", "ignore", 1)]),
        ("<w:balanceSingleByteDoubleByteWidth w:val='invalid'/>", vec![("DOCX_UNSUPPORTED_VALUE", "format_value", "inherit_or_default", 1)]),
    ] {
        let out = convert(Some(&format!("<w:settings xmlns:w='{W}'><w:compat>{children}</w:compat></w:settings>")));
        let actual: Vec<_> = out.warnings.iter().filter_map(|w| match w {
            hwpforge::ops::OpsWarning::Docx(w) if w.part() == Some("options.xml") => Some((w.code(), w.loss(), w.action(), w.count())),
            _ => None,
        }).collect();
        assert_eq!(actual, expected, "{children}");
    }
    assert_used_space(Some(&format!("<w:settings xmlns:w='{W}'><w:compat><w:balanceSingleByteDoubleByteWidth w:val='invalid'/></w:compat></w:settings>")), "1");
}
