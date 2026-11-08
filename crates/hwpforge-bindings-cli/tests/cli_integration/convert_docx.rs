use super::*;
#[path = "../../../hwpforge/tests/support/docx.rs"]
mod docx;
#[path = "../../../hwpforge-smithy-hwpx/tests/support/xml.rs"]
mod xml;

// 이것을 실패시키는 것: 확장자로 거부, ImportMeta 대신 경고 문자열/횟수 요약 출력.
#[test]
fn converts_package_content_with_an_unrelated_extension() {
    let tmp = test_tmp();
    let input = tmp.join("input.bin");
    let output = tmp.join("out.hwpx");
    std::fs::write(
        &input,
        docx::package_with_defaults(
            "<w:p><w:r><w:t>가</w:t></w:r></w:p>",
            "<Relationship Id='opaque0' Type='urn:test:opaque' Target='opaque.xml'/>",
        ),
    )
    .unwrap();
    let (value, stderr, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "../../../hwpforge-bindings-py/tests/data/import-report.json"
    ))
    .unwrap();
    assert_eq!(value, expected);
    assert_eq!(stderr, "");
    let decoded =
        hwpforge_smithy_hwpx::HwpxDecoder::decode(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(decoded.document.sections()[0].paragraphs[0].text_content(), "가");
}

// 이것을 실패시키는 것: 필드 명령 경고의 count를 1로 초기화하거나 첫 위치/후속 생략 표시 누락.
// 이것을 실패시키는 것: 쪽 기본값 경고 삭제 또는 위치 변경.
#[test]
fn text_output_reports_first_location_and_total_occurrences() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    let output = tmp.join("out.hwpx");
    std::fs::write(
        &input,
        docx::package_with_defaults(
            "<w:p><w:fldSimple w:instr='DATE'><w:r><w:t>본문</w:t></w:r></w:fldSimple><w:fldSimple w:instr='TIME'><w:r><w:t>저장 결과</w:t></w:r></w:fldSimple></w:p>",
            "",
        ),
    )
    .unwrap();
    let (stdout, stderr, code) =
        run(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("Generated {} (1 sections, 1 paragraphs)\n", output.display()));
    assert_eq!(stderr, "[convert-docx] DOCX_FIELD_INSTRUCTION_OMITTED: total 2; first body.xml:fldSimple#4; subsequent locations omitted: yes; Field instruction discarded; saved result retained (total 2 occurrences; first example only; subsequent locations omitted)\n[convert-docx] DOCX_PAGE_LAYOUT_APPROXIMATED: total 1; first body.xml:body#0; subsequent locations omitted: no; Missing section page setting uses Core defaults\n[convert-docx] DOCX_PAGE_LAYOUT_APPROXIMATED: total 1; first body.xml:body#0; subsequent locations omitted: no; Missing section page setting uses Core defaults\n");
}

// 이것을 실패시키는 것: 실패 후 출력 파일 생성, 종료 코드/오류 문구 변경, 확장자만으로 수용.
// 이것을 실패시키는 것: 변경 추적/한도 hint를 일반 DOCX 확인 안내로 대체.
#[test]
fn tracked_changes_and_non_docx_content_fail_without_output() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    let output = tmp.join("out.hwpx");
    std::fs::write(&input, docx::package("<w:p><w:ins><w:r><w:t>거부</w:t></w:r></w:ins></w:p>"))
        .unwrap();
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "DOCX_DECODE_FAILED");
    assert_eq!(value["message"], "DOCX part 'body.xml' at ins#4: tracked change 'ins'; accept all changes in Word or LibreOffice and retry");
    assert_eq!(value["hint"], "Accept all changes in Word or LibreOffice and retry");
    assert!(!output.exists());
    std::fs::write(&input, b"not a ZIP").unwrap();
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "DOCX_DECODE_FAILED");
    assert_eq!(value["hint"], "Check that the input is a valid DOCX document");
    assert!(!output.exists());
    let nested = format!("{}<w:p/>{}", "<w:smartTag>".repeat(130), "</w:smartTag>".repeat(130));
    std::fs::write(&input, docx::package(&nested)).unwrap();
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "DOCX_DECODE_FAILED");
    assert_eq!(value["hint"], "Reduce the document resource named in the error and retry");
    assert!(!output.exists());
}

// 이것을 실패시키는 것: 단일 경고에도 이후 위치가 생략되었다고 출력하거나 무경고 입력에 진단 추가(count > 1을 count >= 1로 변경).
// 이것을 실패시키는 것: 일곱 여백 속성이 모두 있어도 page_margins 기본값 경고 추가.
#[test]
fn single_and_no_warning_text_output_keep_their_counts() {
    let tmp = test_tmp();
    let input = tmp.join("in.docx");
    let output = tmp.join("out.hwpx");
    for (body, expected) in [
        ("<w:p><w:r><w:t>본문</w:t></w:r></w:p>", ""),
        ("<w:p><w:fldSimple w:instr='DATE'><w:r><w:t>본문</w:t></w:r></w:fldSimple></w:p>", "[convert-docx] DOCX_FIELD_INSTRUCTION_OMITTED: total 1; first body.xml:fldSimple#4; subsequent locations omitted: no; Field instruction discarded; saved result retained\n"),
    ] {
        std::fs::write(&input, docx::package_with_defaults(&format!("{body}<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>"), "")).unwrap();
        let (stdout, stderr, code) = run(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
        assert_eq!(code, 0);
        assert_eq!(stderr, expected);
        assert_eq!(stdout, format!("Generated {} (1 sections, 1 paragraphs)\n", output.display()));
    }
}

// 이것을 실패시키는 것: 동일 파일 검사 제거 또는 문자열 경로 비교로 교체.
#[test]
fn output_aliases_cannot_overwrite_the_docx_input() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    let bytes = docx::package("<w:p/>");
    std::fs::write(&input, &bytes).unwrap();
    let hard = tmp.join("hard.hwpx");
    std::fs::hard_link(&input, &hard).unwrap();
    let mut outputs = vec![input.clone(), hard];
    #[cfg(unix)]
    {
        let link = tmp.join("link.hwpx");
        std::os::unix::fs::symlink(&input, &link).unwrap();
        outputs.push(link);
    }
    for output in outputs {
        let (value, _, code) =
            run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
        assert_eq!(code, 1);
        assert_eq!(value["code"], "INVALID_OUTPUT_PATH");
        assert_eq!(
            value["message"],
            format!("Output '{}' would overwrite the DOCX input", output.display())
        );
        assert!(value.get("hint").is_none());
        assert_eq!(std::fs::read(&input).unwrap(), bytes);
    }
}

// 이것을 실패시키는 것: DOCX I/O 종료 코드를 1로 복귀하거나 쓰기 오류를 FILE_NOT_FOUND로 분류.
#[test]
fn file_errors_keep_adapter_codes_and_exit_statuses() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    let output = tmp.join("out.hwpx");
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "FILE_NOT_FOUND");
    assert!(value.get("hint").is_none());
    assert!(!output.exists());
    let (value, _, code) =
        run_json(&["convert-docx", tmp.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "FILE_READ_FAILED");
    assert!(value.get("hint").is_none());
    std::fs::write(&input, docx::package("<w:p/>")).unwrap();
    for output in [tmp.join("missing/out.hwpx"), tmp.to_path_buf()] {
        let (value, _, code) =
            run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
        assert_eq!(code, 2);
        assert_eq!(value["code"], "FILE_WRITE_FAILED");
        assert!(value["message"]
            .as_str()
            .unwrap()
            .starts_with(&format!("Cannot write '{}':", output.display())));
        assert!(value.get("hint").is_none());
    }
}

// 이것을 실패시키는 것: 출력 경로 확인 실패를 무시하고 쓰기를 시도.
#[cfg(unix)]
#[test]
fn output_verification_failure_uses_the_write_error_code() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    let output = tmp.join("loop.hwpx");
    std::fs::write(&input, docx::package("<w:p/>")).unwrap();
    std::os::unix::fs::symlink(&output, &output).unwrap();
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "FILE_WRITE_FAILED");
    assert!(value["message"]
        .as_str()
        .unwrap()
        .starts_with(&format!("Cannot verify output '{}':", output.display())));
    assert!(value.get("hint").is_none());
}

// 이것을 실패시키는 것: 동일 파일 검사를 변환 뒤로 옮기기(그러면 DOCX_DECODE_FAILED·종료 2).
#[test]
fn same_file_output_is_rejected_before_decoding_an_invalid_input() {
    let tmp = test_tmp();
    let input = tmp.join("input.docx");
    std::fs::write(&input, b"not a ZIP").unwrap();
    let (value, _, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", input.to_str().unwrap()]);
    assert_eq!(code, 1);
    assert_eq!(value["code"], "INVALID_OUTPUT_PATH");
    assert_eq!(std::fs::read(&input).unwrap(), b"not a ZIP");
}

// 이것을 실패시키는 것: distribute를 Left로 대체·경고 유지, 문단이 다른 paraPr를 참조하거나 HWPX horizontal을 잘못 출력.
#[test]
fn distribute_uses_referenced_hwpx_alignment_without_warnings() {
    use std::io::Read;

    let tmp = test_tmp();
    let input = tmp.join("distribute.docx");
    let output = tmp.join("distribute.hwpx");
    std::fs::write(&input, docx::package_with_defaults("<w:p><w:r><w:t>왼쪽</w:t></w:r></w:p><w:p><w:pPr><w:jc w:val='distribute'/></w:pPr><w:r><w:t>배분 정렬</w:t></w:r></w:p><w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>", "")).unwrap();
    let (value, stderr, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    assert_eq!(value["warnings"], serde_json::json!([]));
    assert_eq!(stderr, "");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut header = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut header).unwrap();
    let mut section = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut section).unwrap();
    let header = xml::Element::parse(&header);
    let section = xml::Element::parse(&section);
    let paragraphs: Vec<_> = section.children.iter().filter(|p| p.name == "hp:p").collect();
    assert_eq!(paragraphs.len(), 2);
    let properties = header.find("hh:paraProperties");
    let actual: Vec<_> = paragraphs
        .iter()
        .map(|p| {
            properties
                .children
                .iter()
                .find(|s| s.name == "hh:paraPr" && s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap()
                .child("hh:align")
                .attr("horizontal")
                .unwrap()
        })
        .collect();
    assert_eq!(actual, ["LEFT", "DISTRIBUTE"]);
}

// 이것을 실패시키는 것: 물리 줄 간격을 반값으로 넘기거나 case/default를 뒤집고 참조하지 않는 모양만 검사.
#[test]
fn physical_spacing_uses_both_referenced_hwpx_branches() {
    use std::io::Read;

    let tmp = test_tmp();
    let input = tmp.join("spacing.docx");
    let output = tmp.join("spacing.hwpx");
    let body: String = [("exact", 400), ("atLeast", 360)]
        .iter()
        .map(|(rule, line)| format!("<w:p><w:pPr><w:spacing w:line='{line}' w:lineRule='{rule}'/></w:pPr><w:r><w:rPr><w:rFonts w:ascii='맑은 고딕' w:eastAsia='맑은 고딕'/><w:sz w:val='22'/></w:rPr><w:t>한글</w:t></w:r></w:p>"))
        .collect();
    std::fs::write(&input, docx::package(&body)).unwrap();
    let (value, stderr, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut header = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut header).unwrap();
    let mut section = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut section).unwrap();
    let header = xml::Element::parse(&header);
    let section = xml::Element::parse(&section);
    let properties = header.find("hh:paraProperties");
    let actual: Vec<_> = section
        .children
        .iter()
        .filter(|p| p.name == "hp:p")
        .map(|p| {
            let shape = properties
                .children
                .iter()
                .find(|s| s.name == "hh:paraPr" && s.attr("id") == p.attr("paraPrIDRef"))
                .unwrap();
            let switch = shape.child("hp:switch");
            let case = switch.child("hp:case").child("hh:lineSpacing");
            let default = switch.child("hp:default").child("hh:lineSpacing");
            (
                case.attr("type").unwrap(),
                case.attr("value").unwrap(),
                default.attr("type").unwrap(),
                default.attr("value").unwrap(),
            )
        })
        .collect();
    assert_eq!(
        actual,
        [("FIXED", "2000", "FIXED", "4000"), ("AT_LEAST", "1902", "AT_LEAST", "3804")]
    );
}

fn font_space_properties(output: &std::path::Path) -> xml::Element {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut header = String::new();
    zip.by_name("Contents/header.xml").unwrap().read_to_string(&mut header).unwrap();
    xml::Element::parse(&header)
}

fn assert_body_font_space(output: &std::path::Path, header: &xml::Element, heights: &[&str]) {
    use std::io::Read;
    let properties = header.find("hh:charProperties");
    let mut zip = zip::ZipArchive::new(std::fs::File::open(output).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut text).unwrap();
    let section = xml::Element::parse(&text);
    let mut references = std::collections::BTreeSet::new();
    for paragraph in &section.children {
        assert_eq!(paragraph.name, "hp:p");
        for run in &paragraph.children {
            if run.name == "hp:run" {
                references.insert(run.attr("charPrIDRef").unwrap());
            }
        }
    }
    assert!(!references.is_empty());
    let mut actual_heights = Vec::new();
    for reference in &references {
        let shape = properties.children.iter().find(|s| s.attr("id") == Some(*reference)).unwrap();
        assert_eq!(shape.attr("useFontSpace"), Some("1"), "body reference {reference}");
        actual_heights.push(shape.attr("height").unwrap());
    }
    actual_heights.sort_unstable();
    assert_eq!(actual_heights, heights);
    // The bridge's built-in styles are unused by these body paragraphs.
    assert!(properties.children.iter().any(|shape| {
        !references.contains(shape.attr("id").unwrap()) && shape.attr("useFontSpace") == Some("0")
    }));
}

// 이것을 실패시키는 것: DOCX 본문 run·빈 문단·문단 끝이 useFontSpace=0인 모양을 참조,
// 본문 참조 대신 미사용 모양만 확인하거나 facade가 bridge 기본값까지 덮어씀.
#[test]
fn docx_font_space_covers_body_references_and_preserves_bridge_defaults() {
    use std::io::{Cursor, Read, Write};
    let tmp = test_tmp();
    let input = tmp.join("font-space.docx");
    let output = tmp.join("font-space.hwpx");
    // No text and no definitions still create a DOCX fallback character shape.
    std::fs::write(&input, docx::package("<w:p/>")).unwrap();
    let (value, stderr, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    let header = font_space_properties(&output);
    assert_body_font_space(&output, &header, &["1000"]);

    let source = docx::package_with_defaults(
        "<w:p/><w:p><w:pPr><w:rPr><w:sz w:val='26'/></w:rPr></w:pPr><w:r><w:rPr><w:sz w:val='24'/></w:rPr><w:t>A B</w:t></w:r><w:r><w:rPr><w:rStyle w:val='character'/></w:rPr><w:t>가 나</w:t></w:r></w:p>", "");
    let mut source = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut package = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let mut part = source.by_index(i).unwrap();
        let mut text = String::new();
        part.read_to_string(&mut text).unwrap();
        if part.name().unwrap() == "styles.xml" {
            text = text.replace("</w:styles>", "<w:style w:type='paragraph' w:styleId='unused'><w:rPr><w:sz w:val='30'/></w:rPr></w:style><w:style w:type='character' w:styleId='character'><w:rPr><w:sz w:val='28'/></w:rPr></w:style></w:styles>");
        }
        package.start_file(part.name().unwrap(), zip::write::SimpleFileOptions::default()).unwrap();
        package.write_all(text.as_bytes()).unwrap();
    }
    std::fs::write(&input, package.finish().unwrap().into_inner()).unwrap();
    let (value, stderr, code) =
        run_json(&["convert-docx", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    let header = font_space_properties(&output);
    let properties = header.find("hh:charProperties");
    assert_body_font_space(&output, &header, &["1000", "1200", "1300", "1400"]);
    // The unused named definition must also survive projection.
    let style = header
        .find("hh:styles")
        .children
        .iter()
        .find(|style| style.attr("name") == Some("import_p_000000"))
        .unwrap();
    let definition = properties
        .children
        .iter()
        .find(|shape| shape.attr("id") == style.attr("charPrIDRef"))
        .unwrap();
    assert_eq!(definition.attr("height"), Some("1500"));
}

// 이것을 실패시키는 것: Blueprint 기본값 또는 공용 HWPX 인코더에서 useFontSpace를 1로 변경.
#[test]
fn markdown_font_space_retains_hancom_defaults() {
    let tmp = test_tmp();
    let input = tmp.join("font-space.md");
    let output = tmp.join("font-space.hwpx");
    std::fs::write(&input, "# 제목\n\n본문 **굵게** *기울임*\n").unwrap();
    let (value, stderr, code) =
        run_json(&["convert", input.to_str().unwrap(), "-o", output.to_str().unwrap()]);
    assert_eq!(code, 0, "{value} {stderr}");
    let header = font_space_properties(&output);
    let properties = header.find("hh:charProperties");
    assert!(!properties.children.is_empty());
    for shape in &properties.children {
        assert_eq!(shape.name, "hh:charPr");
        assert_eq!(shape.attr("useFontSpace"), Some("0"));
    }
}
