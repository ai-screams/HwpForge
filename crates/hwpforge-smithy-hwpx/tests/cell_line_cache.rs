//! One-line cell edits retain the encoder-validated line layout.
#[path = "support/document.rs"]
mod document;
#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

use document::text_para;
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::{
    Document, ImageStore, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{CellSpec, CellTarget, HwpxCellEditor, HwpxEncoder};

// Preserve the cache/geometry test subjects after font qualification tightened.
fn measured_store() -> hwpforge_smithy_hwpx::HwpxStyleStore {
    let mut store = encoding::minimal_store();
    store.replace_font("함초롬돋움", "맑은 고딕");
    store
}

fn fixture(width: i32) -> Vec<u8> {
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("가")], HwpUnit::new(8000).unwrap()),
        TableCell::new(vec![text_para("나")], HwpUnit::new(8000).unwrap()),
    ])]);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("앞"),
            Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            text_para("뒤"),
        ],
        PageSettings::a4(),
    ));
    let bytes =
        HwpxEncoder::encode(&doc.validate().unwrap(), &measured_store(), &ImageStore::new())
            .unwrap();
    let section = package::section_xml(&bytes);
    let section = section.replace("</hp:p>", &format!(r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="{width}" flags="393216"/></hp:linesegarray></hp:p>"#));
    let bytes = package::with_section(&bytes, &section);
    assert_eq!(caches(&bytes).len(), 5);
    bytes
}

fn caches(bytes: &[u8]) -> Vec<std::collections::BTreeMap<String, String>> {
    xml::Element::parse(&package::section_xml(bytes))
        .descendants("hp:lineseg")
        .into_iter()
        .map(|node| node.attributes.clone())
        .collect()
}
fn edit(bytes: &[u8], text: &str) -> Vec<u8> {
    HwpxCellEditor::set_cells(
        bytes,
        &[CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: text.into() }],
    )
    .unwrap()
    .bytes
}

/// 이것을 실패시키는 것: 경계의 <= 를 < 로 변경, 후보 캐시 재부착 삭제, probe 대신 output 비교.
#[test]
fn one_line_width_boundary_preserves_every_cache_field() {
    // 이것을 실패시키는 것: 측정 허용표에서 맑은 고딕 제거.
    let base = fixture(1050);
    let output = edit(&base, "힣");
    assert_eq!(caches(&output), caches(&base));
    assert_eq!(
        xml::Element::parse(&package::section_xml(&output)).find("hp:tc").find("hp:t").text,
        "힣"
    );
}

/// 이것을 실패시키는 것: 폭 비교 삭제, 허용 문자 검사 삭제, 빈 문자열 허용.
#[test]
fn unproven_text_keeps_the_existing_invalidation() {
    for (width, text) in
        [(1049, "힣"), (40000, "A"), (40000, "%"), (40000, "１"), (40000, "\u{a0}"), (40000, "")]
    {
        assert_eq!(caches(&edit(&fixture(width), text)).len(), 2, "{width}: {text:?}");
    }
}

/// 이것을 실패시키는 것: 정리 전 문자열로 폭 계산, 공백 trim, 한글 가중치를 100으로 변경.
#[test]
fn cleaned_text_and_ascii_widths_are_used_without_trimming() {
    let base = fixture(1050);
    assert_eq!(caches(&edit(&base, "\u{1}힣")), caches(&base));
    assert_eq!(caches(&edit(&base, " 힣 ")).len(), 2);
    for text in ["0", "9", " ", ".", ",", "-", "(", ")", "/", ":"] {
        let base = fixture(1000);
        assert_eq!(caches(&edit(&base, text)), caches(&base), "{text}");
    }
}

/// 이것을 실패시키는 것: 원본 subList/paraPr 속성 검사 각각 삭제.
#[test]
fn original_layout_attributes_must_all_be_explicitly_supported() {
    for (part, from, to) in [
        ("Contents/header.xml", "fontLineHeight=\"0\"", "fontLineHeight=\"1\""),
        ("Contents/header.xml", "condense=\"0\"", "condense=\"1\""),
        ("Contents/header.xml", "eAsianEng=\"0\"", "eAsianEng=\"1\""),
        ("Contents/header.xml", "eAsianNum=\"0\"", "eAsianNum=\"1\""),
        ("Contents/header.xml", "lineWrap=\"BREAK\"", "lineWrap=\"SQUEEZE\""),
        ("Contents/section0.xml", "textDirection=\"HORIZONTAL\"", "textDirection=\"VERTICAL\""),
        ("Contents/section0.xml", "lineWrap=\"BREAK\"", "lineWrap=\"SQUEEZE\""),
    ] {
        let base = package::splice_part(&fixture(40000), part, from, to);
        assert_eq!(caches(&edit(&base, "힣")).len(), 2, "{from}");
    }
}

/// 이것을 실패시키는 것: probe를 원본 문서로 만들거나 pageBreak 차이를 비교에서 지우기.
#[test]
fn nontext_paragraph_change_still_invalidates_the_host_and_tail() {
    let base = fixture(40000);
    let section = package::section_xml(&base);
    let pos = section.find(">가</hp:t>").unwrap();
    let start = section[..pos].rfind("<hp:p ").unwrap();
    let mut section = section;
    let end = start + section[start..].find('>').unwrap();
    let tag = section[start..end].replace("pageBreak=\"0\"", "pageBreak=\"1\"");
    section.replace_range(start..end, &tag);
    let base = package::with_section(&base, &section);
    assert_eq!(caches(&edit(&base, "힣")).len(), 2);
}

/// 이것을 실패시키는 것: 첫 paraPr 대신 id=0 정의를 선택하기.
#[test]
fn header_definition_order_takes_precedence_over_ids() {
    let base = fixture(40000);
    let header = package::part(&base, "Contents/header.xml");
    let start = header.find("<hh:paraPr ").unwrap();
    let end = start + header[start..].find("</hh:paraPr>").unwrap() + "</hh:paraPr>".len();
    let first = &header[start..end];
    let definitions = format!(
        "{}{}",
        first
            .replace("id=\"0\"", "id=\"1\"")
            .replace("fontLineHeight=\"0\"", "fontLineHeight=\"1\""),
        first
    );
    let base = package::splice_part(&base, "Contents/header.xml", first, &definitions);
    assert_eq!(caches(&edit(&base, "힣")).len(), 2);
}

/// 이것을 실패시키는 것: 후보 표 ordinal 대신 존재하지 않는 ordinal로 조회해 캐시/텍스트 복원을 생략하기.
#[test]
fn mixed_edits_keep_only_proven_cells_in_either_label_order() {
    let base = fixture(40000);
    let good =
        CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: "힣".into() };
    let bad = CellSpec { table: 0, target: CellTarget::RightOf("가".into()), text: "A".into() };
    for specs in [[good.clone(), bad.clone()], [bad.clone(), good.clone()]] {
        let output = HwpxCellEditor::set_cells(&base, &specs).unwrap().bytes;
        assert_eq!(caches(&output).len(), 2);
        let tree = xml::Element::parse(&package::section_xml(&output));
        let cells = &tree.find("hp:tr").children;
        assert_eq!(cells[0].find("hp:t").text, "힣");
        assert_eq!(cells[1].find("hp:t").text, "A");
        assert_eq!(cells[0].find("hp:lineseg").attr("horzsize"), Some("40000"));
    }
}

/// 이것을 실패시키는 것: 슬롯 수 불일치 fallback 삭제, 다문단 칸을 후보로 인정하기.
#[test]
fn contracting_a_multiparagraph_cell_uses_section_fallback() {
    let base = fixture(40000);
    let section = package::section_xml(&base);
    let pos = section.find(">가</hp:t>").unwrap();
    let start = section[..pos].rfind("<hp:p ").unwrap();
    let end = pos + section[pos..].find("</hp:p>").unwrap() + "</hp:p>".len();
    let extra = section[start..end].replace(">가</hp:t>", ">다</hp:t>");
    let mut section = section;
    section.insert_str(end, &extra);
    let base = package::with_section(&base, &section);
    assert_eq!(caches(&base).len(), 6);
    assert!(caches(&edit(&base, "힣")).is_empty());
    let specs: Vec<_> = (0..2)
        .map(|col| CellSpec {
            table: 0,
            target: CellTarget::At(GridCoord::new(0, col)),
            text: "힣".into(),
        })
        .collect();
    assert!(caches(&HwpxCellEditor::set_cells(&base, &specs).unwrap().bytes).is_empty());
}

fn nested_fixture(textbox: bool) -> Vec<u8> {
    let unit = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(vec![text_para("가")], unit)])]);
    let inner = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let run = if textbox {
        Run::control(
            hwpforge_core::Control::TextBox {
                frame: hwpforge_core::ObjectFrame::new(),
                text_margin: None,
                paragraphs: vec![inner],
                caption: None,
                width: unit,
                height: unit,
                placement: None,
                style: None,
                text_vertical_align: Default::default(),
            },
            CharShapeIndex::new(0),
        )
    } else {
        Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(vec![inner], unit)])]),
            CharShapeIndex::new(0),
        )
    };
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("앞"),
            Paragraph::with_runs(vec![run], ParaShapeIndex::new(0)),
            text_para("뒤"),
        ],
        PageSettings::a4(),
    ));
    let bytes =
        HwpxEncoder::encode(&doc.validate().unwrap(), &measured_store(), &ImageStore::new())
            .unwrap();
    let section = package::section_xml(&bytes).replace("</hp:p>", r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="1050" flags="393216"/></hp:linesegarray></hp:p>"#);
    let bytes = package::with_section(&bytes, &section);
    assert_eq!(caches(&bytes).len(), 5);
    bytes
}

/// 이것을 실패시키는 것: changed를 baseline/probe 대신 baseline/output으로 비교하기.
#[test]
fn nested_table_and_textbox_table_keep_the_complete_host_chain() {
    for textbox in [false, true] {
        let base = nested_fixture(textbox);
        let output = HwpxCellEditor::set_cells(
            &base,
            &[CellSpec {
                table: usize::from(!textbox),
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "힣".into(),
            }],
        )
        .unwrap()
        .bytes;
        assert_eq!(caches(&output), caches(&base));
    }
}

/// 이것을 실패시키는 것: 빈 원본/혼합 run/끝 표시/캐시 부재/다줄/textpos 거부를 각각 제거.
#[test]
fn cached_cell_shape_rejections_preserve_old_invalidation() {
    let base = fixture(40000);
    let section = package::section_xml(&base);
    let text_at = section.find(">가</hp:t>").unwrap();
    let start = section[..text_at].rfind("<hp:p ").unwrap();
    let end = text_at + section[text_at..].find("</hp:p>").unwrap() + "</hp:p>".len();
    let paragraph = &section[start..end];
    let cache_at = paragraph.find("<hp:linesegarray>").unwrap();
    let cache_end = paragraph.find("</hp:linesegarray>").unwrap() + "</hp:linesegarray>".len();
    let variants = [
        paragraph.replace(">가</hp:t>", "></hp:t>"),
        paragraph.replace("</hp:run>", "</hp:run><hp:run charPrIDRef=\"0\"><hp:t>나</hp:t></hp:run>"),
        paragraph.replace("</hp:run>", "</hp:run><hp:run charPrIDRef=\"0\"/>"),
        format!("{}{}", &paragraph[..cache_at], &paragraph[cache_end..]),
        paragraph.replace(">가</hp:t>", ">가나</hp:t>").replace("</hp:linesegarray>", "<hp:lineseg textpos=\"1\" vertpos=\"1100\" vertsize=\"1000\" textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" horzsize=\"40000\" flags=\"393216\"/></hp:linesegarray>"),
        paragraph.replace(">가</hp:t>", ">가나</hp:t>").replace("textpos=\"0\"", "textpos=\"1\""),
    ];
    for (i, variant) in variants.iter().enumerate() {
        let input = package::with_section(&base, &section.replace(paragraph, variant));
        assert!(caches(&input).len() >= 4);
        assert_eq!(caches(&edit(&input, "힣")).len(), 2, "variant {i}");
    }
}

/// 이것을 실패시키는 것: stamp가 자기 output 대신 pristine을 probe로 넘기기,
/// ZIP 항목 이름·내용을 빼거나 이름순 대신 역순으로 해시하기.
#[test]
fn stamp_output_bytes_remain_identical() {
    use hwpforge_smithy_hwpx::stamp::{plan, HwpxStamper, StampAction, StampSpec};
    use sha2::{Digest, Sha256};
    let base = package::splice(&fixture(40000), ">가</hp:t>", ">( )</hp:t>");
    // This byte fingerprint covers stamp, whose source font must stay unchanged.
    let base = package::splice_part(&base, "Contents/header.xml", "맑은 고딕", "함초롬돋움");
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&base).unwrap();
    let specs: Vec<_> = plan(&decoded.document)
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "값".into(), hint: None },
        })
        .collect();
    assert_eq!(specs.len(), 1);
    let bytes = HwpxStamper::stamp(&base, &specs).unwrap().bytes;
    let request = hwpforge_smithy_hwpx::stamp::StampRequestV2 {
        schema_version: hwpforge_smithy_hwpx::stamp::STAMP_MAP_VERSION,
        source_sha256: Sha256::digest(&base).iter().map(|b| format!("{b:02x}")).collect(),
        text: specs,
        cells: vec![],
    };
    assert_eq!(HwpxStamper::stamp_v2(&base, &request).unwrap().bytes, bytes);

    assert_eq!(
        package_digest(&bytes),
        "a6bb1ae0e24886d66dc2ef59eae12acc6dc632ced8957911349a394d0aa14514"
    );
}

/// 이것을 실패시키는 것: 다른 host의 실패 편집까지 probe에서 되돌리거나 모든 후보 캐시를 버리기.
#[test]
fn mixed_edits_in_different_hosts_preserve_only_eligible_flows() {
    let mut decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture(40000)).unwrap();
    let host = decoded.document.sections()[0].paragraphs[1].clone();
    let tail = decoded.document.sections()[0].paragraphs[2].clone();
    decoded.document.sections_mut()[0].paragraphs.extend([host, tail]);
    let input = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert_eq!(caches(&input).len(), 9);
    let good =
        CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: "힣".into() };
    let bad = CellSpec { table: 1, target: CellTarget::At(GridCoord::new(0, 0)), text: "A".into() };
    for specs in [[good.clone(), bad.clone()], [bad.clone(), good.clone()]] {
        let output = HwpxCellEditor::set_cells(&input, &specs).unwrap().bytes;
        assert_eq!(caches(&output).len(), 6);
    }
}

/// 이것을 실패시키는 것: 공백 전용 가중치 삭제, 한글 가중치를 100으로 변경.
#[test]
fn spaced_hangul_at_measured_size_preserves_every_cache_field() {
    let base = package::splice_part(
        &fixture(8544),
        "Contents/header.xml",
        "height=\"1000\"",
        "height=\"1300\"",
    );
    assert!(package::part(&base, "Contents/header.xml").contains("snapToGrid=\"1\""));
    assert_eq!(caches(&edit(&base, "편집 확인 칸")), caches(&base));
    for (width, kept) in [(8125, true), (8124, false)] {
        let input = package::splice(&base, "horzsize=\"8544\"", &format!("horzsize=\"{width}\""));
        assert_eq!(caches(&edit(&input, "편집 확인 칸")).len(), if kept { 5 } else { 2 });
    }
}

/// 이것을 실패시키는 것: c.cell을 0으로 고정, 같은 표에서 첫 후보만 복원.
#[test]
fn second_cell_and_two_candidates_preserve_all_caches() {
    let base = fixture(40000);
    for columns in [vec![1], vec![0, 1]] {
        let specs: Vec<_> = columns
            .iter()
            .map(|&col| CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(0, col)),
                text: "힣".into(),
            })
            .collect();
        let output = HwpxCellEditor::set_cells(&base, &specs).unwrap().bytes;
        assert_eq!(caches(&output), caches(&base));
        let tree = xml::Element::parse(&package::section_xml(&output));
        let cells = &tree.find("hp:tr").children;
        for col in columns {
            assert_eq!(cells[col as usize].find("hp:t").text, "힣");
        }
    }
}

fn encode_cached(doc: Document<hwpforge_core::Draft>) -> Vec<u8> {
    let bytes =
        HwpxEncoder::encode(&doc.validate().unwrap(), &measured_store(), &ImageStore::new())
            .unwrap();
    let section = package::section_xml(&bytes).replace("</hp:p>", r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="40000" flags="393216"/></hp:linesegarray></hp:p>"#);
    package::with_section(&bytes, &section)
}

/// 이것을 실패시키는 것: 해석된 row_idx 대신 요청 격자 행으로 캐시 복원, cell_idx 대신 격자 열 사용.
#[test]
fn merged_anchor_and_covered_coordinate_preserve_caches() {
    let unit = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![
        TableRow::new(vec![
            TableCell::with_span(vec![text_para("가")], unit, 1, 3),
            TableCell::new(vec![text_para("나")], unit),
        ]),
        TableRow::new(vec![TableCell::with_span(vec![text_para("다")], unit, 1, 2)]),
        TableRow::new(vec![]),
    ]);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("앞"),
            Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            text_para("뒤"),
        ],
        PageSettings::a4(),
    ));
    let base = encode_cached(doc);
    assert_eq!(caches(&base).len(), 6);
    for row in [1, 2] {
        let result = HwpxCellEditor::set_cells(
            &base,
            &[CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(row, 1)),
                text: "힣".into(),
            }],
        )
        .unwrap();
        assert_eq!(result.outcome.cells[0].anchor, GridCoord::new(1, 1));
        assert_eq!(
            result.outcome.cells[0].resolution,
            if row == 1 {
                hwpforge_smithy_hwpx::CellResolution::Exact
            } else {
                hwpforge_smithy_hwpx::CellResolution::CoveredToAnchor
            }
        );
        assert_eq!(caches(&result.bytes), caches(&base));
        let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&result.bytes).unwrap();
        let table =
            decoded.document.sections()[0].paragraphs[1].runs[0].content.as_table().unwrap();
        assert_eq!(table.rows[1].cells[0].paragraphs[0].text_content(), "힣");
    }
}

/// 이것을 실패시키는 것: 각주 표 후보 복원을 생략.
#[test]
fn footnote_table_preserves_its_flow_caches() {
    let unit = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(vec![text_para("가")], unit)])]);
    let inner = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("앞"),
            Paragraph::with_runs(
                vec![Run::control(
                    hwpforge_core::Control::footnote(vec![inner, text_para("각주 뒤")]),
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            ),
            text_para("뒤"),
        ],
        PageSettings::a4(),
    ));
    let raw = encode_cached(doc);
    // The source encoder refuses the injected note-number head cache; retain its valid caches.
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&raw).unwrap();
    let base = HwpxEncoder::encode_with_diagnostics(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap()
    .bytes;
    assert_eq!(caches(&base).len(), 5);
    let output = edit(&base, "힣");
    assert_eq!(caches(&output), caches(&base));
    assert_eq!(
        xml::Element::parse(&package::section_xml(&output)).find("hp:tc").find("hp:t").text,
        "힣"
    );
}

/// 이것을 실패시키는 것: 둘째 구역의 후보를 첫 구역에서 조회 또는 복원.
#[test]
fn second_section_candidate_preserves_every_cache_field() {
    let mut decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture(40000)).unwrap();
    decoded.document.add_section(decoded.document.sections()[0].clone());
    let base = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let output = HwpxCellEditor::set_cells(
        &base,
        &[CellSpec { table: 1, target: CellTarget::At(GridCoord::new(0, 1)), text: "힣".into() }],
    )
    .unwrap()
    .bytes;
    for section in [0, 1] {
        let path = format!("Contents/section{section}.xml");
        let original = package::with_section(&base, &package::part(&base, &path));
        let actual = package::with_section(&output, &package::part(&output, &path));
        assert_eq!(caches(&original).len(), 5);
        assert_eq!(caches(&actual), caches(&original));
    }
    let tree = xml::Element::parse(&package::part(&output, "Contents/section1.xml"));
    assert_eq!(tree.find("hp:tr").children[1].find("hp:t").text, "힣");
}

/// 이것을 실패시키는 것: 글자 격자/원고지 또는 축소 상대크기+양수 자간 거부 삭제.
#[test]
fn unmeasured_spacing_and_section_grids_invalidate() {
    let base = fixture(40000);
    for attr in ["charGrid", "wonggojiFormat"] {
        for replacement in [format!("{attr}=\"1\""), String::new()] {
            let input = package::splice(&base, &format!("{attr}=\"0\""), &replacement);
            assert_eq!(caches(&edit(&input, "힣")).len(), 2, "{attr}: {replacement}");
        }
    }
    let header = package::part(&base, "Contents/header.xml");
    let shape = |tag: &str, value: &str| {
        let start = header.find(&format!("<hh:{tag} ")).unwrap();
        let end = start + header[start..].find("/>").unwrap() + 2;
        header[start..end]
            .replace("=\"100\"", &format!("=\"{value}\""))
            .replace("=\"0\"", &format!("=\"{value}\""))
    };
    let rel = shape("relSz", "90");
    let spacing = shape("spacing", "1");
    let mut input = base;
    for (tag, replacement) in [("relSz", rel), ("spacing", spacing)] {
        let start = header.find(&format!("<hh:{tag} ")).unwrap();
        let end = start + header[start..].find("/>").unwrap() + 2;
        input =
            package::splice_part(&input, "Contents/header.xml", &header[start..end], &replacement);
    }
    assert_eq!(caches(&edit(&input, "힣")).len(), 2);
}

#[path = "support/small_stack.rs"]
mod small_stack;
/// 이것을 실패시키는 것: 원본 Tag 스택의 깊이 제한 삭제.
#[test]
fn deep_unknown_xml_keeps_set_cells_successful() {
    small_stack::small_stack_child("deep_unknown_xml_keeps_set_cells_successful", || {
        let nested = format!("{}{}", "<x>".repeat(1_000_000), "</x>".repeat(1_000_000));
        let base = fixture(40000);
        let header = package::splice_part(
            &base,
            "Contents/header.xml",
            "</hh:head>",
            &format!("{nested}</hh:head>"),
        );
        assert!(HwpxCellEditor::set_cells(
            &header,
            &[CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "힣".into()
            }]
        )
        .is_ok());
        let nested_cell = format!("{}{}", "<x>".repeat(1000), "</x>".repeat(1000));
        let cell = package::splice(&base, "</hp:tc>", &format!("{nested_cell}</hp:tc>"));
        assert!(HwpxCellEditor::set_cells(
            &cell,
            &[CellSpec {
                table: 0,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "힣".into()
            }]
        )
        .is_ok());
    });
}

fn package_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut entries = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().unwrap().to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        assert!(entries.insert(name, data).is_none());
    }
    let mut hash = Sha256::new();
    for (name, data) in entries {
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((data.len() as u64).to_le_bytes());
        hash.update(data);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// 이것을 실패시키는 것: 모든 후보의 격자를 첫 구역에서 읽기.
#[test]
fn section_grid_rejection_uses_the_candidates_own_section() {
    let mut decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&fixture(40000)).unwrap();
    decoded.document.add_section(decoded.document.sections()[0].clone());
    let base = HwpxEncoder::encode_with_options(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
        hwpforge_smithy_hwpx::EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let base =
        package::splice_part(&base, "Contents/section1.xml", "charGrid=\"0\"", "charGrid=\"1\"");
    for table in [0, 1] {
        let output = HwpxCellEditor::set_cells(
            &base,
            &[CellSpec { table, target: CellTarget::At(GridCoord::new(0, 0)), text: "힣".into() }],
        )
        .unwrap()
        .bytes;
        for section in [0, 1] {
            let path = format!("Contents/section{section}.xml");
            let actual = package::with_section(&output, &package::part(&output, &path));
            assert_eq!(caches(&actual).len(), if table == 1 && section == 1 { 2 } else { 5 });
        }
    }
}

// 이것을 실패시키는 것: 글자 효과/글꼴 공백의 공통 후보 판정 우회.
#[test]
fn effects_and_font_space_remove_cell_self_caches() {
    let base = fixture(40000);
    for (from, to) in
        [("useFontSpace=\"0\"", "useFontSpace=\"1\""), ("</hh:charPr>", "<hh:bold/></hh:charPr>")]
    {
        let bytes = package::splice_part(&base, "Contents/header.xml", from, to);
        assert_eq!(caches(&edit(&bytes, "힣")).len(), 2);
    }
}

// 이것을 실패시키는 것: 언어별 수치 손실의 입력 admission 거부 생략.
#[test]
fn per_language_loss_still_rejects_public_cell_edits() {
    let base = package::splice_part(
        &fixture(40000),
        "Contents/header.xml",
        "latin=\"100\"",
        "latin=\"99\"",
    );
    let result = HwpxCellEditor::set_cells(
        &base,
        &[CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 0)), text: "힣".into() }],
    );
    assert!(result.unwrap_err().to_string().contains("char_shape_per_language"));
}

#[test]
fn compatibility_settings_exclude_unproved_single_line_optimization() {
    // 이것을 실패시키는 것: Word/미지원 호환 설정을 한 줄 폭 증명에 허용.
    for (target, flags) in [("MS_WORD", ""), ("FUTURE", ""), ("HWP201X", "<hh:future/>")] {
        let bytes = package::splice_part(
            &fixture(1050),
            "Contents/header.xml",
            r#"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument>"#,
            &format!(
                r#"<hh:compatibleDocument targetProgram="{target}"><hh:layoutCompatibility>{flags}</hh:layoutCompatibility></hh:compatibleDocument>"#
            ),
        );
        let out = edit(&bytes, "힣");
        let tree = xml::Element::parse(&package::section_xml(&out));
        assert!(tree.find("hp:tc").descendants("hp:lineseg").is_empty());
    }
}

#[test]
fn equivalent_hancom_compatibility_keeps_proven_cell_cache() {
    // 이것을 실패시키는 것: 구조상 같은 한컴 설정을 문자열 완전 일치로 판정.
    let bytes = package::splice_part(
        &fixture(1050),
        "Contents/header.xml",
        r#"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument>"#,
        "<hh:compatibleDocument targetProgram = 'HWP201X'>\n <hh:layoutCompatibility ></hh:layoutCompatibility>\n</hh:compatibleDocument>",
    );
    let out = edit(&bytes, "힣");
    assert_eq!(
        xml::Element::parse(&package::section_xml(&out))
            .find("hp:tc")
            .descendants("hp:lineseg")
            .len(),
        1
    );
}
