//! Cell replacement shares the preserving text patch's output contract.
#[path = "support/document.rs"]
mod document;
#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::{
    Document, PageSettings, Paragraph, Run, RunContent, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    CellSpec, CellTarget, EncodeWarning, HwpxCellEditor, HwpxDecoder, HwpxPatcher,
};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

fn fixture(sections: usize) -> Vec<u8> {
    let mut doc = Document::new();
    for _ in 0..sections {
        let table = Table::new(vec![TableRow::new(vec![
            TableCell::new(vec![document::text_para("가")], HwpUnit::new(8000).unwrap()),
            TableCell::new(vec![document::text_para("나")], HwpUnit::new(8000).unwrap()),
        ])]);
        doc.add_section(Section::with_paragraphs(
            vec![
                Paragraph::with_runs(
                    vec![Run::table(table, CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                ),
                document::text_para("뒤"),
            ],
            PageSettings::a4(),
        ));
    }
    encoding::encode(doc)
}
fn spec(table: usize, col: u32, text: &str) -> CellSpec {
    CellSpec { table, target: CellTarget::At(GridCoord::new(0, col)), text: text.into() }
}
fn payloads(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut e = zip.by_index(i).unwrap();
            let name = e.name().unwrap().to_string();
            let mut data = Vec::new();
            e.read_to_end(&mut data).unwrap();
            (name, data)
        })
        .collect()
}
fn with_entries(bytes: &[u8], entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut parts = payloads(bytes);
    parts.extend(entries.iter().map(|(p, b)| ((*p).into(), b.to_vec())));
    write_payloads(parts)
}
fn write_payloads(parts: BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in parts {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn patch(base: &[u8], specs: &[CellSpec]) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(base).unwrap();
    let mut bytes = base.to_vec();
    let mut ordinal = 0;
    for (section, source) in decoded.document.sections().iter().enumerate() {
        let preservation = HwpxPatcher::export_section_preservation(base, section, source).unwrap();
        let mut replacement = source.clone();
        for paragraph in &mut replacement.paragraphs {
            for run in &mut paragraph.runs {
                let RunContent::Table(table) = &mut run.content else { continue };
                for edit in specs.iter().filter(|edit| edit.table == ordinal) {
                    let CellTarget::At(coord) = edit.target else { panic!("coordinate") };
                    table.rows[0].cells[coord.col as usize].paragraphs[0].runs[0].content =
                        RunContent::Text(edit.text.clone());
                }
                ordinal += 1;
            }
        }
        bytes = HwpxPatcher::patch_section_preserving(
            &bytes,
            section,
            &replacement,
            None,
            Some(&preservation),
        )
        .unwrap();
    }
    bytes
}
fn assert_patch(base: &[u8], specs: &[CellSpec], output: &[u8]) {
    assert_eq!(payloads(output), payloads(&patch(base, specs)));
}

// 이것을 실패시키는 것: 공유 wire run의 후손에 Complete 출처를 요구하기.
#[test]
fn set_cell_preserves_cells_inside_a_shared_wire_run() {
    for empty in [false, true] {
        let base = package::splice(&fixture(1), "<hp:tbl ", "<hp:t>앞</hp:t><hp:tbl ");
        let base = package::splice(&base, "</hp:tbl>", "</hp:tbl><hp:t/>");
        let base = if empty { package::splice(&base, "<hp:t>가</hp:t>", "") } else { base };
        let edits = [spec(0, 0, "새값")];
        let output = HwpxCellEditor::set_cells(&base, &edits).unwrap();
        assert_eq!(
            package::section_xml(&output.bytes),
            package::section_xml(&patch(&base, &edits))
        );
        assert_eq!(
            package::part(&output.bytes, "Contents/header.xml"),
            package::part(&base, "Contents/header.xml")
        );
        assert_patch(&base, &edits, &output.bytes);
    }
}

// 이것을 실패시키는 것: 보존 계획보다 먼저 재인코드 admission을 실행하기.
#[test]
fn set_cell_preserves_unrelated_payloads() {
    let base = with_entries(
        &fixture(1),
        &[("Custom/payload.bin", b"unchanged"), ("Preview/PrvText.txt", b"old preview")],
    );
    let descriptor = package::part(&base, "Contents/content.hpf");
    let container = package::part(&base, "META-INF/container.xml");
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "</opf:manifest>",
        r#"<opf:item id="preview" href="Preview/PrvText.txt" media-type="text/plain"/></opf:manifest>"#,
    );
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "</opf:spine>",
        r#"<opf:itemref idref="preview" linear="yes"/></opf:spine>"#,
    );
    let base = package::splice_part(
        &base,
        "META-INF/container.xml",
        "</ocf:rootfiles>",
        r#"<ocf:rootfile full-path="Preview/PrvText.txt" media-type="text/plain"/></ocf:rootfiles>"#,
    );
    let edits = [spec(0, 0, "새값")];
    let result = HwpxCellEditor::set_cells_with_diagnostics(&base, &edits).unwrap();
    let parts = payloads(&result.value.bytes);
    assert_eq!(parts["Custom/payload.bin"], b"unchanged");
    assert!(!parts.contains_key("Preview/PrvText.txt"));
    assert_eq!(parts["Contents/content.hpf"], descriptor.as_bytes());
    assert_eq!(parts["META-INF/container.xml"], container.as_bytes());
    assert_eq!(
        result
            .encode_warnings
            .iter()
            .filter(|w| matches!(w, EncodeWarning::PreviewRemoved { .. }))
            .count(),
        1
    );
    assert_patch(&base, &edits, &result.value.bytes);
}

// 이것을 실패시키는 것: 원문 빈 run을 재인코드하여 t로 정규화하기.
#[test]
fn set_cell_edits_each_empty_wire_shape() {
    let base = fixture(1);
    for run in [
        r#"<hp:run charPrIDRef="0"/>"#,
        r#"<hp:run charPrIDRef="0"></hp:run>"#,
        r#"<hp:run charPrIDRef="0"><hp:t/></hp:run>"#,
        r#"<hp:run charPrIDRef="0"><hp:t></hp:t></hp:run>"#,
    ] {
        let base =
            package::splice(&base, r#"<hp:run charPrIDRef="0"><hp:t>가</hp:t></hp:run>"#, run);
        let noop = HwpxCellEditor::set_cells(&base, &[spec(0, 0, "")]).unwrap();
        assert_eq!(noop.bytes, base);
        assert!(noop.outcome.cells[0].cleared);
        let filled = HwpxCellEditor::set_cells(&base, &[spec(0, 0, "<&가")]).unwrap();
        let tree = xml::Element::parse(&package::section_xml(&filled.bytes));
        let cell = tree.find("hp:tc");
        assert_eq!(cell.descendants("hp:t").len(), 1);
        assert_eq!(cell.find("hp:t").text, "<&가");
        assert_patch(&base, &[spec(0, 0, "<&가")], &filled.bytes);
        let cleared = HwpxCellEditor::set_cells(&filled.bytes, &[spec(0, 0, "")]).unwrap();
        let tree = xml::Element::parse(&package::section_xml(&cleared.bytes));
        let cell = tree.find("hp:tc");
        assert_eq!(cell.descendants("hp:p").len(), 1);
        assert_eq!(cell.descendants("hp:run").len(), 1);
        assert!(cleared.outcome.cells[0].cleared);
    }
}

// 이것을 실패시키는 것: 보존 replacement를 새 Paragraph로 만들기.
#[test]
fn set_cell_keeps_paragraph_and_table_attributes() {
    let base = package::splice(&fixture(1), "pageBreak=\"0\"", "pageBreak=\"1\"");
    let base = package::splice(&base, "columnBreak=\"0\"", "columnBreak=\"1\"");
    let base = package::splice(&base, "merged=\"0\"", "merged=\"1\"");
    let source = package::section_xml(&base);
    let start = source.find("<hp:sz ").unwrap();
    let end = start + source[start..].find("/>").unwrap() + 2;
    let base = package::splice(
        &base,
        &source[start..end],
        &source[start..end].replace("height=\"0\"", "height=\"5764\""),
    );
    let base = package::splice(&base, "vertAlign=\"CENTER\"", "vertAlign=\"BOTTOM\"");
    let source = package::section_xml(&base);
    let start = source.find("<hp:cellSz ").unwrap();
    let end = start + source[start..].find("/>").unwrap() + 2;
    let base = package::splice(
        &base,
        &source[start..end],
        &source[start..end].replace("height=\"0\"", "height=\"777\""),
    );
    let output = HwpxCellEditor::set_cells(&base, &[spec(0, 0, "새값")]).unwrap();
    let original = xml::Element::parse(&package::section_xml(&base));
    let tree = xml::Element::parse(&package::section_xml(&output.bytes));
    assert_eq!(
        tree.find("hp:tc").find("hp:p").attributes,
        original.find("hp:tc").find("hp:p").attributes
    );
    assert_eq!(
        tree.find("hp:tbl").find("hp:sz").attributes,
        original.find("hp:tbl").find("hp:sz").attributes
    );
    assert_eq!(tree.find("hp:cellSz").attributes, original.find("hp:cellSz").attributes);
    assert_eq!(tree.find("hp:tbl").find("hp:sz").attr("height"), Some("5764"));
    assert_eq!(tree.find("hp:cellSz").attr("height"), Some("777"));
    assert_eq!(tree.find("hp:subList").attr("vertAlign"), Some("BOTTOM"));
    assert_patch(&base, &[spec(0, 0, "새값")], &output.bytes);
}

// 이것을 실패시키는 것: 범위 밖 칸과 구역별 또는 칸별로 보존/재인코드를 섞기.
#[test]
fn set_cell_uses_one_route_for_the_batch() {
    for separate in [false, true] {
        let base = fixture(if separate { 2 } else { 1 });
        let base = package::splice(&base, "pageBreak=\"0\"", "pageBreak=\"1\"");
        let base = package::two_text_runs(&base, "<hp:t>나</hp:t>");
        let base = if separate {
            package::splice_part(
                &base,
                "Contents/section1.xml",
                "<hp:t>나</hp:t>",
                "<hp:t>A</hp:t></hp:run><hp:run charPrIDRef=\"1\"><hp:t>B</hp:t>",
            )
        } else {
            base
        };
        let first = spec(0, 0, "새값");
        let single = HwpxCellEditor::set_cells(&base, std::slice::from_ref(&first)).unwrap();
        assert_eq!(
            xml::Element::parse(&package::section_xml(&single.bytes))
                .find("hp:tc")
                .find("hp:p")
                .attr("pageBreak"),
            Some("1")
        );
        let second = spec(usize::from(separate), 1, "다른값");
        for edits in [[first.clone(), second.clone()], [second.clone(), first.clone()]] {
            let output = HwpxCellEditor::set_cells(&base, &edits).unwrap();
            assert_eq!(
                xml::Element::parse(&package::section_xml(&output.bytes))
                    .find("hp:tc")
                    .find("hp:p")
                    .attr("pageBreak"),
                Some("0")
            );
            assert_eq!(output.outcome.cells.len(), 2);
        }
    }
}

// 이것을 실패시키는 것: 구역 내 순번을 문서 전체 표 순번 대신 사용하기.
#[test]
fn set_cell_preserves_sections_in_package_order() {
    let mut decoded = HwpxDecoder::decode(&fixture(3)).unwrap();
    for section in decoded.document.sections_mut() {
        let sibling = section.paragraphs[0].clone();
        let second = section.paragraphs[0].runs[0].clone();
        section.paragraphs[0].runs.push(second);
        section.paragraphs.push(sibling);
    }
    let generated = hwpforge_smithy_hwpx::HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    let third = package::part(&generated, "Contents/section2.xml");
    let mut parts = payloads(&generated);
    for (from, to) in [
        ("Contents/section0.xml", "Contents/section7.xml"),
        ("Contents/section1.xml", "Contents/section11.xml"),
        ("Contents/section2.xml", "Contents/section25.xml"),
    ] {
        let xml = parts.remove(from).unwrap();
        parts.insert(to.into(), xml);
    }
    let base = with_entries(&write_payloads(parts), &[("Preview/PrvText.txt", b"old")]);
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "Contents/section0.xml",
        "Contents/section7.xml",
    );
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "Contents/section1.xml",
        "Contents/section11.xml",
    );
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "Contents/section2.xml",
        "Contents/section25.xml",
    );
    let base = package::splice_part(
        &base,
        "Contents/section7.xml",
        "뒤</hp:t>",
        "뒤</hp:t><hp:imaginary/>",
    );
    let base = package::splice_part(
        &base,
        "Contents/section11.xml",
        "뒤</hp:t>",
        "뒤</hp:t><hp:imaginary/>",
    );
    let base =
        package::splice_part(&base, "Contents/content.hpf", "idref=\"section0\"", "idref=\"SWAP\"");
    let base = package::splice_part(
        &base,
        "Contents/content.hpf",
        "idref=\"section1\"",
        "idref=\"section0\"",
    );
    let base =
        package::splice_part(&base, "Contents/content.hpf", "idref=\"SWAP\"", "idref=\"section1\"");
    let edits = [
        spec(5, 1, "끝"),
        spec(0, 0, "처음"),
        spec(3, 0, "중간"),
        spec(1, 1, "같은host"),
        spec(2, 0, "형제"),
        spec(4, 1, "둘째"),
    ];
    let output = HwpxCellEditor::set_cells_with_diagnostics(&base, &edits).unwrap();
    assert_patch(&base, &edits, &output.value.bytes);
    assert_eq!(
        xml::Element::parse(&package::part(&output.value.bytes, "Contents/section11.xml"))
            .find("hp:t")
            .text,
        "처음"
    );
    assert_eq!(
        output
            .encode_warnings
            .iter()
            .filter(|w| matches!(w, EncodeWarning::PreviewRemoved { .. }))
            .count(),
        1
    );
    assert_eq!(output.decode_warnings.len(), 2);
    assert_eq!(package::part(&output.value.bytes, "Contents/section25.xml"), third);
    assert_eq!(output.decode_warnings, HwpxDecoder::decode(&base).unwrap().warnings);
}

// 이것을 실패시키는 것: 정리한 값을 patch에 넘기지 않거나 줄바꿈 정규화를 생략하기.
#[test]
fn set_cell_cleans_values_once() {
    for (text, expected, children, count) in [
        (" A \tB ", " A B ", vec!["hp:tab"], 0),
        ("a\nb", "ab", vec!["hp:lineBreak"], 0),
        ("a\rb", "ab", vec!["hp:lineBreak"], 0),
        ("a\r\nb", "ab", vec!["hp:lineBreak"], 0),
        ("a\r\u{1}\nb", "ab", vec!["hp:lineBreak"], 1),
        ("\0\u{1}\u{b}\u{c}\u{fffe}\u{ffff}", "", vec![], 6),
        ("\u{1f}\u{a0}", "", vec!["hp:fwSpace", "hp:nbSpace"], 0),
        ("<&가", "<&가", vec![], 0),
    ] {
        let base = fixture(1);
        let output =
            HwpxCellEditor::set_cells_with_diagnostics(&base, &[spec(0, 0, text)]).unwrap();
        let tree = xml::Element::parse(&package::section_xml(&output.value.bytes));
        let t = tree.find("hp:tc").find("hp:t");
        assert_eq!(t.text, expected);
        // 이것을 실패시키는 것: LF를 문자열 앞으로 옮겨 mixed content 순서를 바꾸기.
        use xml::Content::{Child, Text};
        let content = match text {
            " A \tB " => vec![Text(" A ".into()), Child(0), Text("B ".into())],
            "a\nb" | "a\rb" | "a\r\nb" | "a\r\u{1}\nb" => {
                vec![Text("a".into()), Child(0), Text("b".into())]
            }
            "\0\u{1}\u{b}\u{c}\u{fffe}\u{ffff}" => vec![],
            "\u{1f}\u{a0}" => vec![Child(0), Child(1)],
            "<&가" => vec![Text("<&가".into())],
            _ => panic!("missing literal content expectation"),
        };
        assert_eq!(t.content, content);
        assert_eq!(t.children.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), children);
        let warnings = &output.encode_warnings;
        if count == 0 {
            assert!(warnings.is_empty());
        } else {
            assert!(
                matches!(&warnings[..], [EncodeWarning::XmlForbiddenCharsRemoved { part, location, count: actual }] if part == "Contents/section0.xml" && location == "table 0 cell (0, 0)" && *actual == count)
            );
        }
    }
}

// 이것을 실패시키는 것: 무변경 값도 patch 실행과 Preview 제거 대상으로 삼기.
#[test]
fn set_cell_preserves_normalized_noops() {
    for (original, requested, count) in
        [("가", "가", 0), ("가", "가\u{1}", 1), ("a<hp:lineBreak/>b", "a\r\nb", 0)]
    {
        let base = package::splice(&fixture(1), "가</hp:t>", &format!("{original}</hp:t>"));
        let base = with_entries(&base, &[("Preview/PrvText.txt", b"preview")]);
        let result =
            HwpxCellEditor::set_cells_with_diagnostics(&base, &[spec(0, 0, requested)]).unwrap();
        assert_eq!(result.value.bytes, base);
        assert_eq!(result.encode_warnings.len(), count);
    }
}

// 이것을 실패시키는 것: 슬롯 밖 미해석 내용을 지우거나 입력 손실을 일괄 거부하기.
#[test]
fn set_cell_keeps_unmodeled_siblings_in_the_target_paragraph() {
    for (from, to) in [
        ("가</hp:t>", "가</hp:t><hp:imaginary/>"),
        ("</hp:run></hp:p>", "</hp:run><hp:imaginary/></hp:p>"),
    ] {
        let base = package::splice(&fixture(1), from, to);
        let decoded = HwpxDecoder::decode(&base).unwrap();
        assert!(decoded.warnings.iter().any(|w| w.is_content_loss()));
        let output =
            HwpxCellEditor::set_cells_with_diagnostics(&base, &[spec(0, 0, "새값")]).unwrap();
        assert_eq!(output.decode_warnings, decoded.warnings);
        assert_eq!(
            package::section_xml(&output.value.bytes),
            package::section_xml(&base).replace("가</hp:t>", "새값</hp:t>")
        );
        assert_patch(&base, &[spec(0, 0, "새값")], &output.value.bytes);
    }
}

// 이것을 실패시키는 것: requested 좌표를 물리 칸 배열 인덱스로 쓰기.
#[test]
fn set_cell_resolves_labels_and_covered_targets_from_the_original() {
    let unit = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::with_span(vec![document::text_para("성명：")], unit, 2, 1),
            TableCell::new(vec![document::text_para("가")], unit),
        ]),
        TableRow::new(
            (0..3).map(|_| TableCell::new(vec![document::text_para("반복")], unit)).collect(),
        ),
    ]);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::table(table, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let base = encoding::encode(doc);
    let edits = [
        CellSpec {
            table: 0, target: CellTarget::At(GridCoord::new(0, 1)), text: "새 라벨".into()
        },
        CellSpec {
            table: 0, target: CellTarget::RightOf(" 성명 ".into()), text: "오른쪽".into()
        },
        CellSpec { table: 0, target: CellTarget::Below("성명：".into()), text: "아래".into() },
    ];
    let output = HwpxCellEditor::set_cells(&base, &edits).unwrap();
    assert_eq!(output.outcome.cells[0].requested, GridCoord::new(0, 1));
    assert_eq!(output.outcome.cells[0].anchor, GridCoord::new(0, 0));
    assert_eq!(
        output.outcome.cells[0].resolution,
        hwpforge_smithy_hwpx::CellResolution::CoveredToAnchor
    );
    let tree = xml::Element::parse(&package::section_xml(&output.bytes));
    let rows = tree.descendants("hp:tr");
    assert_eq!(rows[0].children[0].find("hp:t").text, "새 라벨");
    assert_eq!(rows[0].children[1].find("hp:t").text, "오른쪽");
    assert_eq!(rows[1].children[0].find("hp:t").text, "아래");
    assert_eq!(rows[1].children[1].find("hp:t").text, "반복");
}
