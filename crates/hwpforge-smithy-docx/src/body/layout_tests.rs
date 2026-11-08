use super::tests::{package, W};
use crate::{decode_docx, DocxDecoded};

fn read(body: &str) -> DocxDecoded {
    decode_docx(&package(body, &[])).unwrap()
}
fn texts(d: &DocxDecoded) -> Vec<Vec<String>> {
    d.document
        .sections()
        .iter()
        .map(|s| s.paragraphs.iter().map(|p| p.text_content()).collect())
        .collect()
}
fn losses(d: &DocxDecoded, loss: &str) -> u64 {
    d.warnings.iter().filter(|w| w.loss() == loss).map(|w| w.count()).sum()
}

// 이것을 실패시키는 것: 선행·후행·연속 빈 조각 제거, pageBreakBefore 복제,
// 앞뒤 간격·첫 줄·keep·끝 모양을 모든 조각에 복사, StyleRef 삭제.
#[test]
fn split_edges_distribute_properties_and_preserve_style_identity() {
    let styles = format!("<w:styles xmlns:w='{W}'><w:style w:type='paragraph' w:styleId='p'><w:pPr><w:spacing w:before='20' w:after='40'/><w:ind w:left='60' w:right='80' w:firstLine='10'/><w:keepNext/><w:keepLines/><w:widowControl/></w:pPr></w:style></w:styles>");
    for content in [
        "<w:r><w:br w:type='page'/><w:t>A</w:t><w:br w:type='page'/><w:br w:type='page'/></w:r>",
        "<w:r><w:br w:type='page'/></w:r><w:r><w:t>A</w:t></w:r><w:r><w:br w:type='page'/></w:r><w:r><w:br w:type='page'/></w:r>",
    ] {
        for before in [false, true] {
            let pb = if before { "<w:pageBreakBefore/>" } else { "" };
            let body = format!("<w:p><w:pPr><w:pStyle w:val='p'/>{pb}<w:rPr><w:color w:val='123456'/></w:rPr></w:pPr>{content}</w:p>");
            let d = decode_docx(&package(&body, &[("styles", "styles.xml", &styles)])).unwrap();
            assert_eq!(texts(&d), [vec!["", "A", "", ""]]);
            let p = &d.document.sections()[0].paragraphs;
            assert_eq!(p.iter().map(|p| p.page_break).collect::<Vec<_>>(), [before, true, true, true]);
            assert!(p.iter().all(|p| !p.column_break && p.style_ref == Some(hwpforge_foundation::StyleRef::new(0))));
            assert_eq!(p.iter().map(|p| p.runs.len()).collect::<Vec<_>>(), [1, 1, 1, 1]);
            let s: Vec<_> = p.iter().map(|p| &d.registry.para_shapes[p.para_shape_id.get()]).collect();
            assert_eq!(s.iter().map(|s| s.space_before.as_i32()).collect::<Vec<_>>(), [100, 0, 0, 0]);
            assert_eq!(s.iter().map(|s| s.space_after.as_i32()).collect::<Vec<_>>(), [0, 0, 0, 200]);
            assert_eq!(s.iter().map(|s| s.indent_first_line.as_i32()).collect::<Vec<_>>(), [50, 0, 0, 0]);
            assert_eq!(s.iter().map(|s| s.keep_with_next).collect::<Vec<_>>(), [false, false, false, true]);
            assert!(s.iter().all(|s| !s.keep_lines_together && !s.widow_orphan && s.indent_left.as_i32() == 300 && s.indent_right.as_i32() == 400 && s.list.is_none()));
            assert!(p[..3].iter().all(|p| p.end_char_shape.is_none()));
            let mark = p[3].end_char_shape.unwrap();
            assert_eq!(d.registry.char_shapes[mark.get()].color, hwpforge_foundation::Color::from_rgb(0x12, 0x34, 0x56));
            assert_eq!(losses(&d, "paragraph_split"), 1);
        }
    }
    let d = read(
        "<w:p><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t><w:br w:type='page'/></w:r></w:p>",
    );
    assert_eq!(texts(&d), [vec!["A", "B", ""]]);
    assert_eq!(
        d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
        [false, true, true]
    );
}

// 이것을 실패시키는 것: 빈 조각에 기본 모양 부여, 분할 후 delta ID를 registry ID로 오해.
#[test]
fn empty_fragments_use_boundary_character_format() {
    let d = read("<w:p><w:r><w:rPr><w:b/></w:rPr><w:br w:type='page'/></w:r><w:r><w:rPr><w:i/></w:rPr><w:br w:type='page'/></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["", "", ""]]);
    let shapes: Vec<_> = d.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| &d.registry.char_shapes[p.runs[0].char_shape_id.get()])
        .collect();
    assert_eq!(
        shapes.iter().map(|s| (s.bold, s.italic)).collect::<Vec<_>>(),
        [(true, false), (false, true), (false, true)]
    );
}

// 이것을 실패시키는 것: 문단 sectPr를 다음 구역에 적용, 빈 최종 구역 삭제, 같은 치수 구역 병합.
#[test]
fn section_boundaries_follow_last_fragment_and_keep_empty_final_section() {
    let layout = "<w:sectPr><w:pgSz w:w='15840' w:h='12240' w:orient='landscape'/></w:sectPr>";
    let d = read(&format!("<w:p><w:pPr>{layout}</w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>{layout}"));
    assert_eq!(texts(&d), [vec!["A", ""], vec!["B"]]);
    for s in d.document.sections() {
        assert_eq!(
            (
                s.page_settings.width.as_i32(),
                s.page_settings.height.as_i32(),
                s.page_settings.landscape
            ),
            (61200, 79200, true)
        );
    }
    let d = read(&format!("<w:p><w:pPr>{layout}</w:pPr><w:r><w:t>A</w:t></w:r></w:p>{layout}"));
    assert_eq!(texts(&d), [vec!["A"], vec![""]]);
    assert_eq!(d.document.sections()[1].paragraphs[0].runs.len(), 1);
}

// 이것을 실패시키는 것: orient를 치수보다 우선, 세로 순서 정규화 제거, 정사각 기본 방향에 경고 추가.
#[test]
fn page_geometry_normalizes_orientation_and_margins() {
    for (w, h, orient, width, height, landscape, warnings) in [
        (12240, 15840, "landscape", 61200, 79200, false, 1),
        (15840, 12240, "portrait", 61200, 79200, true, 1),
        (12240, 15840, "portrait", 61200, 79200, false, 0),
        (15840, 12240, "landscape", 61200, 79200, true, 0),
        (12240, 12240, "landscape", 61200, 61200, true, 0),
        (12240, 12240, "portrait", 61200, 61200, false, 0),
        (12000, 12000, "", 60000, 60000, false, 0),
        (15840, 12240, "", 61200, 79200, true, 1),
    ] {
        let orient = if orient.is_empty() { String::new() } else { format!("w:orient='{orient}'") };
        let d = read(&format!("<w:sectPr><w:pgSz w:w='{w}' w:h='{h}' {orient}/><w:pgMar w:top='100' w:right='200' w:bottom='300' w:left='400' w:header='50' w:footer='60'/></w:sectPr>"));
        let p = &d.document.sections()[0].page_settings;
        assert_eq!((p.width.as_i32(), p.height.as_i32(), p.landscape), (width, height, landscape));
        assert_eq!(
            [
                p.margin_top,
                p.margin_right,
                p.margin_bottom,
                p.margin_left,
                p.header_margin,
                p.footer_margin
            ]
            .map(|v| v.as_i32()),
            [250, 1000, 300, 2000, 250, 1200]
        );
        assert_eq!(losses(&d, "page_orientation"), warnings);
    }
}

// 이것을 실패시키는 것: 구조 수치 오류를 국소 서식 대체, 본문 크기 검사 생략.
#[test]
fn invalid_structural_dimensions_fail_with_observed_value_and_limit() {
    for value in
        ["oops", "0", "-1", "20000001", "999999999999999999999999999999999999999999999999999999"]
    {
        let error = decode_docx(&package(
            &format!("<w:sectPr><w:pgSz w:w='{value}' w:h='15840'/></w:sectPr>"),
            &[],
        ))
        .unwrap_err()
        .to_string();
        assert!(
            error.contains(value) && error.contains("100000000") && error.contains("pgSz"),
            "{error}"
        );
    }
    for margins in ["w:left='12240' w:right='0'", "w:top='15840' w:bottom='0'", "w:header='-1'"] {
        assert!(decode_docx(&package(
            &format!("<w:sectPr><w:pgSz w:w='12240' w:h='15840'/><w:pgMar {margins}/></w:sectPr>"),
            &[]
        ))
        .is_err());
    }
}

// 이것을 실패시키는 것: 단 나누기 누락, 다단·구역 시작 의미 손실을 무음 처리.
#[test]
fn layout_approximations_preserve_column_breaks() {
    for kind in ["continuous", "oddPage", "evenPage", "nextPage"] {
        let d = read(&format!("<w:p><w:r><w:t>A</w:t><w:br w:type='column'/><w:t>B</w:t></w:r></w:p><w:sectPr><w:type w:val='{kind}'/><w:cols w:num='2'/></w:sectPr>"));
        assert_eq!(texts(&d), [vec!["A", "B"]]);
        assert!(d.document.sections()[0].paragraphs[1].page_break);
        assert_eq!(losses(&d, "column_break"), 1);
        assert_eq!(losses(&d, "columns"), 1);
        assert_eq!(losses(&d, "section_start"), u64::from(kind != "nextPage"));
    }
}

fn indexed(body: &str) -> (Vec<super::DecodedSection>, super::Relations) {
    let header = format!("<w:hdr xmlns:w='{W}'/>");
    let footer = format!("<w:ftr xmlns:w='{W}'/>");
    let bytes = package(body, &[("header", "h.xml", &header), ("footer", "f.xml", &footer)]);
    let mut package = crate::package::DocxPackage::open(&bytes).unwrap();
    let mut relations = super::Relations::default();
    for relation in &package.relationships {
        relations.admit(relation, "text/body.xml").unwrap();
    }
    let mut warnings = Vec::new();
    let mut resolver =
        crate::format::Resolver::new(crate::format::Definitions::default(), &mut warnings).unwrap();
    let bytes = package.read_main().unwrap();
    let sections = super::scan_formatted(
        &bytes,
        "text/body.xml",
        &mut package.text_budget,
        &mut warnings,
        &mut resolver,
        &relations,
    )
    .unwrap();
    (sections, relations)
}

// 이것을 실패시키는 것: default 상속 제거, first/even 상속, 원본 ID와 출력 조각 ID 혼동,
// 방향 정규화 후 세로 폭으로 본문 폭 계산, 명시 미해결 참조에 이전 default 재주입.
#[test]
fn private_section_index_keeps_origins_references_and_physical_extents() {
    let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let (sections, _) = indexed(&format!("<w:p><w:pPr><w:sectPr xmlns:r='{r}'><w:headerReference w:type='default' r:id='r0'/><w:headerReference w:type='first' r:id='r0'/><w:footerReference w:type='default' r:id='r1'/><w:footerReference w:type='even' r:id='r1'/><w:pgSz w:w='15840' w:h='12240' w:orient='landscape'/><w:pgMar w:left='100' w:right='200' w:top='300' w:bottom='400' w:gutter='50'/></w:sectPr></w:pPr><w:r><w:t>A</w:t><w:br w:type='column'/></w:r></w:p><w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:sectPr xmlns:r='{r}'><w:headerReference w:type='default' r:id='missing'/></w:sectPr>"));
    assert_eq!(sections.len(), 3);
    let first = sections[0].references;
    assert!(first.headers[0].unwrap().is_some());
    assert_eq!(first.headers[0], first.headers[1]);
    assert!(first.footers[2].unwrap().is_some());
    assert_eq!(sections[1].references.headers, [first.headers[0], None, None]);
    assert_eq!(sections[1].references.footers, [first.footers[0], None, None]);
    assert_eq!(sections[2].references.headers, [Some(None), None, None]);
    assert_eq!(
        (sections[0].content_width.as_i32(), sections[0].content_height.as_i32()),
        (77450, 57700)
    );
    assert_eq!(
        sections[0]
            .origins
            .iter()
            .map(|o| (o.paragraph, o.list_item, o.fragment, o.cause))
            .collect::<Vec<_>>(),
        [(1, 1, 0, None), (1, 1, 1, Some(super::Break::Column))]
    );
    assert_eq!(sections[1].origins[0].paragraph, 2);
}

// 이것을 실패시키는 것: 관계 ID 길이 검사 우회, 잘못된 종류·누락 참조 수용, 중복 참조 덮어쓰기.
#[test]
fn section_references_are_bounded_and_do_not_resolve_wrong_kinds() {
    let r = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let header = format!("<w:hdr xmlns:w='{W}'/>");
    for id in ["r0".to_owned(), "missing".to_owned(), "x".repeat(4096)] {
        let d = decode_docx(&package(&format!("<w:sectPr xmlns:r='{r}'><w:footerReference w:type='default' r:id='{id}'/></w:sectPr>"), &[("header", "h.xml", &header)])).unwrap();
        assert_eq!(losses(&d, "header_footer_reference"), 1);
    }
    let error = decode_docx(&package(
        &format!(
            "<w:sectPr xmlns:r='{r}'><w:headerReference w:type='default' r:id='{}'/></w:sectPr>",
            "x".repeat(4097)
        ),
        &[],
    ))
    .unwrap_err();
    assert!(matches!(error, crate::DocxError::Limit { observed: 4097, limit: 4096, .. }));
    let duplicate = "<w:headerReference w:type='default'/>".repeat(2);
    assert!(decode_docx(&package(&format!("<w:sectPr>{duplicate}</w:sectPr>"), &[])).is_err());
}

// 이것을 실패시키는 것: 숨겨진 필드 명령의 나누기 적용, 분할 없는 문단의 keep/간격 초기화.
#[test]
fn unsplit_and_invisible_breaks_preserve_original_properties() {
    let d = read("<w:p><w:pPr><w:keepNext/><w:keepLines/><w:widowControl/><w:spacing w:before='20' w:after='40'/></w:pPr><w:r><w:fldChar w:fldCharType='begin'/><w:br w:type='page'/><w:fldChar w:fldCharType='separate'/><w:t>A</w:t><w:fldChar w:fldCharType='end'/></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["A"]]);
    let p = &d.document.sections()[0].paragraphs[0];
    let shape = &d.registry.para_shapes[p.para_shape_id.get()];
    assert_eq!(
        (
            shape.space_before.as_i32(),
            shape.space_after.as_i32(),
            shape.keep_with_next,
            shape.keep_lines_together,
            shape.widow_orphan
        ),
        (100, 200, true, true, true)
    );
    assert_eq!(losses(&d, "paragraph_split"), 0);
    assert!(decode_docx(&package("<w:p><w:br w:type='page'/></w:p>", &[])).is_err());
}

// 이것을 실패시키는 것: 알 수 없는 쪽 속성 무음 처리, cols 수량 오류 수용,
// 중첩 sectPr 허용, 제본 설정 손실 누락, 부속 root 불일치 참조 연결.
#[test]
fn unsupported_layout_values_and_relationship_roots_are_explicit() {
    let d = read("<w:sectPr w:future='1'><w:pgSz w:w='12240' w:h='15840' w:orient='future'/><w:type w:val='future'/><w:pgMar w:future='1'/><w:future><w:pgSz w:w='oops'/></w:future><x:future xmlns:x='urn:future'/><w:headerReference w:type='future'/></w:sectPr>");
    assert_eq!(losses(&d, "page_orientation"), 1);
    assert_eq!(losses(&d, "page_attribute"), 2);
    assert_eq!(losses(&d, "page_property"), 2);
    assert_eq!(losses(&d, "header_footer_reference"), 1);
    for num in ["0", "-1", "oops", "4294967296"] {
        assert!(decode_docx(&package(
            &format!("<w:sectPr><w:cols w:num='{num}'/></w:sectPr>"),
            &[]
        ))
        .is_err());
    }
    assert!(decode_docx(&package("<w:p><w:pPr><w:sectPr/><w:sectPr/></w:pPr></w:p>", &[])).is_err());
    let settings =
        format!("<w:settings xmlns:w='{W}'><w:mirrorMargins/><w:gutterAtTop/></w:settings>");
    let d = decode_docx(&package("<w:p/>", &[("settings", "settings.xml", &settings)])).unwrap();
    assert_eq!(losses(&d, "binding_layout"), 2);
    let wrong = format!("<w:ftr xmlns:w='{W}'/>");
    let d = decode_docx(&package("<w:sectPr><w:headerReference xmlns:r='http://schemas.openxmlformats.org/officeDocument/2006/relationships' w:type='default' r:id='r0'/></w:sectPr>", &[("header", "h.xml", &wrong)])).unwrap();
    assert_eq!(losses(&d, "header_footer_reference"), 1);
}

// 이것을 실패시키는 것: 한 원본 run의 손실을 출력 조각 수만큼 집계.
#[test]
fn split_run_diagnostics_count_source_runs_once() {
    let d = read("<w:p><w:r><w:rPr><w:rStyle w:val='missing'/></w:rPr><w:br w:type='page'/><w:t>A</w:t><w:br w:type='page'/><w:br w:type='page'/></w:r></w:p>");
    assert_eq!(losses(&d, "font_size"), 1);
    assert_eq!(losses(&d, "style_reference"), 1);
}

// 이것을 실패시키는 것: 원본 끝 모양을 첫 조각에 남기거나 계속 조각에서 제거.
#[test]
fn paragraph_end_shape_nine_is_only_on_the_last_fragment() {
    let mut body = String::from("<w:p/>");
    for color in 1..9 {
        body.push_str(&format!(
            "<w:p><w:r><w:rPr><w:color w:val='{color:06x}'/></w:rPr><w:t>X</w:t></w:r></w:p>"
        ));
    }
    body.push_str("<w:p><w:pPr><w:rPr><w:color w:val='000009'/></w:rPr></w:pPr><w:r><w:br w:type='page'/><w:t>A</w:t><w:br w:type='page'/><w:br w:type='page'/></w:r></w:p>");
    let d = read(&body);
    assert_eq!(
        d.document.sections()[0].paragraphs[9..]
            .iter()
            .map(|p| p.end_char_shape.map(|c| c.get()))
            .collect::<Vec<_>>(),
        [None, None, None, Some(9)]
    );
}

// 이것을 실패시키는 것: body의 직접 자식이 아닌 문단의 sectPr를 구역 경계로 사용.
#[test]
fn retained_wrappers_do_not_promote_nested_sections() {
    let d = read("<w:sdt><w:sdtContent><w:p><w:pPr><w:sectPr><w:pgSz w:w='oops'/></w:sectPr></w:pPr><w:r><w:t>A</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:r><w:t>B</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["A", "B"]]);
    assert_eq!(losses(&d, "nested_section"), 1);
}

// 이것을 실패시키는 것: 분할·빈 구역·run 한도 제거, 대기 조각 차감을 다음 문단 한도 검사 뒤로 이동.
#[test]
fn fragment_and_empty_section_growth_use_document_limits() {
    use super::*;
    let mut warnings = Vec::new();
    let mut resolver = Resolver::new(Definitions::default(), &mut warnings).unwrap();
    let mut body = Body {
        part: "body.xml",
        main: true,
        root: "document",
        root_matches: true,
        body_seen: true,
        final_section_seen: false,
        stack: vec![],
        fields: vec![],
        paragraphs: vec![],
        sections: vec![],
        origins: vec![],
        breaks: vec![],
        trailing_break: None,
        split_at: None,
        boundary_input: None,
        source_paragraphs: 1,
        paragraph_top_level: true,
        output_paragraphs: MAX_PARAGRAPHS,
        layout: None,
        paragraph_layout: None,
        relations: None,
        paragraph: Some(Paragraph::new(ParaShapeIndex::new(0))),
        run: Some((String::new(), true)),
        run_diagnosed: false,
        run_split: false,
        diagnose_runs: vec![],
        resolver: Some(&mut resolver),
        direct_para: Delta::default(),
        direct_run: Delta::default(),
        mark: None,
        mark_style: None,
        paragraph_style: None,
        run_style: None,
        runs: 0,
        ordinal: 0,
        warnings: &mut warnings,
    };
    let at = Location { name: Rc::from("br"), ordinal: 1 };
    assert!(matches!(
        body.split(false, &at),
        Err(DocxError::Limit { observed: 1_000_001, limit: 1_000_000, .. })
    ));
    body.output_paragraphs -= 1;
    body.split(false, &at).unwrap();
    assert_eq!(body.output_paragraphs, 1_000_000);
    assert!(body.split(false, &at).is_err());
    assert!(matches!(
        body.close_section(Layout::default(), at.clone()),
        Err(DocxError::Limit { observed: 1_000_001, limit: 1_000_000, .. })
    ));
    body.runs = MAX_RUNS;
    assert!(matches!(
        body.ensure_fragment_run(&at),
        Err(DocxError::Limit { observed: 4_000_001, limit: 4_000_000, .. })
    ));
    // Inject the boundary count, without allocating a million paragraphs.
    body.runs = 0;
    body.output_paragraphs = MAX_PARAGRAPHS - 1;
    body.paragraph = Some(Paragraph::new(ParaShapeIndex::new(0)));
    body.breaks.clear();
    body.split_at = None;
    body.diagnose_runs.clear();
    body.boundary_input = None;
    body.split(false, &at).unwrap();
    body.ensure_fragment_run(&at).unwrap();
    body.finish_paragraph().unwrap();
    assert_eq!(body.paragraphs.len(), 2);
    assert!(body.trailing_break.is_some());
    body.stack.push(Frame {
        node: Node::Body,
        blocked: false,
        in_body: true,
        properties: false,
        location: at.clone(),
    });
    let element = || Element {
        namespace: Rc::from(W_NS.1),
        namespace_id: W_NS.0,
        name: "p".into(),
        attributes: vec![],
    };
    body.start(element()).unwrap();
    assert_eq!(body.output_paragraphs, 1_000_000);
    assert_eq!(body.paragraphs.len(), 1);
    assert_eq!(body.origins.len(), 1);
    assert!(body.paragraph.as_ref().unwrap().page_break);
    body.ensure_fragment_run(&at).unwrap();
    body.finish_paragraph().unwrap();
    assert!(matches!(
        body.start(element()),
        Err(DocxError::Limit { observed: 1_000_001, limit: 1_000_000, .. })
    ));
}

// 이것을 실패시키는 것: 여러 빈 원본 run을 빈 조각에 그대로 복사하거나 경계 전 빈 run의 모양을 선택.
#[test]
fn empty_source_runs_collapse_to_one_effective_boundary_run() {
    let d = read("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t/></w:r><w:r><w:t/></w:r><w:r><w:rPr><w:i/></w:rPr><w:br w:type='page'/><w:br w:type='page'/></w:r><w:r><w:t/></w:r><w:r><w:rPr><w:b/></w:rPr><w:t/></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["", "", ""]]);
    let p = &d.document.sections()[0].paragraphs;
    assert_eq!(p.iter().map(|p| p.runs.len()).collect::<Vec<_>>(), [1, 1, 1]);
    assert_eq!(
        p.iter()
            .map(|p| {
                let s = &d.registry.char_shapes[p.runs[0].char_shape_id.get()];
                (s.bold, s.italic)
            })
            .collect::<Vec<_>>(),
        [(false, true), (false, true), (true, false)]
    );
}

// 이것을 실패시키는 것: equalWidth=false 또는 col 자식 감지 누락, 같은 cols 중복 경고.
#[test]
fn unequal_columns_warn_once_at_first_exclusion() {
    for columns in [
        "<w:cols w:equalWidth='0'><w:col w:w='3000' w:space='720'/><w:col w:w='6000'/></w:cols>",
        "<w:cols w:equalWidth='0'/>",
        "<w:cols w:equalWidth='false'/>",
        "<w:cols w:equalWidth='off'/>",
        "<w:cols><w:col w:w='3000'><w:future/></w:col><w:col w:w='6000'/></w:cols>",
        "<w:cols w:num='2'><w:col w:w='3000'/><w:col w:w='6000'/></w:cols>",
    ] {
        let d = read(&format!("<w:sectPr>{columns}</w:sectPr>"));
        let warnings: Vec<_> = d
            .warnings
            .iter()
            .filter(|w| w.loss() == "columns" || w.loss() == "page_property")
            .map(|w| (w.code(), w.loss(), w.action(), w.count()))
            .collect();
        assert_eq!(
            warnings,
            [("DOCX_PAGE_LAYOUT_APPROXIMATED", "columns", "single_column", 1)],
            "{columns}"
        );
    }
    let d = read("<w:sectPr><w:cols w:equalWidth='1' w:num='1'/></w:sectPr>");
    assert_eq!(losses(&d, "columns"), 0);
}

// 이것을 실패시키는 것: 지원 leaf의 자식 무음 제외, 제외 하위 트리 재진단·속성 주입.
#[test]
fn section_property_subtrees_use_leaf_and_excluded_states() {
    for property in [
        "<w:pgSz w:w='11906' w:h='16838'><x:future xmlns:x='urn:future'><w:pgSz w:w='oops'/></x:future></w:pgSz>",
        "<w:pgMar><w:future><w:pgSz w:w='oops'/></w:future></w:pgMar>",
        "<w:type><w:future><w:pgSz w:w='oops'/></w:future></w:type>",
        "<w:headerReference w:type='default'><w:future><w:pgSz w:w='oops'/></w:future></w:headerReference>",
        "<w:cols><w:future><w:col/><w:pgSz w:w='oops'/></w:future></w:cols>",
        "<w:future><w:future><w:pgSz w:w='oops'/></w:future></w:future>",
        "<x:future xmlns:x='urn:future'><w:pgSz w:w='oops'/></x:future>",
    ] {
        let d = read(&format!("<w:sectPr>{property}</w:sectPr>"));
        assert_eq!(losses(&d, "page_property"), 1, "{property}");
        assert_eq!(losses(&d, "columns"), 0);
    }
    let d = read("<w:sectPr><w:headerReference w:type='future'><w:future><w:pgSz w:w='oops'/></w:future></w:headerReference></w:sectPr>");
    assert_eq!(losses(&d, "header_footer_reference"), 1);
    assert_eq!(losses(&d, "page_property"), 0);
}

// 이것을 실패시키는 것: 보이는 나누기의 필드 결과 상태 갱신 누락 또는 숨긴 명령의 상태 갱신.
#[test]
fn break_only_field_results_are_present_in_simple_complex_and_nested_fields() {
    for kind in ["page", "column"] {
        let br = format!("<w:r><w:br w:type='{kind}'/></w:r>");
        let simple = format!("<w:fldSimple w:instr='PAGE'>{br}</w:fldSimple>");
        let complex = format!("<w:r><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/></w:r>{br}<w:r><w:fldChar w:fldCharType='end'/></w:r>");
        for field in [simple.clone(), complex.clone(), format!("<w:fldSimple w:instr='OUTER'>{simple}</w:fldSimple>"), format!("<w:fldSimple w:instr='OUTER'>{complex}</w:fldSimple>"), format!("<w:r><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/></w:r>{complex}<w:r><w:fldChar w:fldCharType='end'/></w:r>")] {
            let d = read(&format!("<w:p>{field}</w:p>"));
            assert_eq!(texts(&d), [vec!["", ""]]);
            assert_eq!(d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(), [false, true]);
            assert_eq!(losses(&d, "field_result"), 0);
            assert!(losses(&d, "field_function") >= 1);
        }
    }
    let d = read("<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:br w:type='page'/><w:fldChar w:fldCharType='separate'/><w:fldChar w:fldCharType='end'/></w:r></w:p>");
    assert_eq!(texts(&d), [vec![""]]);
    assert_eq!(losses(&d, "field_result"), 1);
}

// 이것을 실패시키는 것: 표 셀 텍스트 펼침, 제외 표의 구역·나누기 적용 또는 추가 배치 경고.
#[test]
fn excluded_tables_do_not_contribute_text_sections_or_breaks() {
    let d = read("<w:p><w:r><w:t>A</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:pPr><w:sectPr><w:pgSz w:w='oops'/></w:sectPr></w:pPr><w:r><w:t>CELL</w:t><w:br w:type='page'/><w:br w:type='column'/></w:r></w:p></w:tc></w:tr></w:tbl><w:p><w:r><w:t>B</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["A", "B"]]);
    assert_eq!(
        d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
        [false, false]
    );
    let w = d.warnings.iter().find(|w| w.loss() == "table").unwrap();
    assert_eq!((w.code(), w.action(), w.count()), ("DOCX_CONTENT_OMITTED", "omit", 1));
    for loss in ["nested_section", "cell_break", "paragraph_split", "column_break"] {
        assert_eq!(losses(&d, loss), 0);
    }
    assert!(matches!(
        decode_docx(&package("<w:tbl><w:tr><w:tc><w:p><w:ins/></w:p></w:tc></w:tr></w:tbl>", &[])),
        Err(crate::DocxError::TrackedChange { .. })
    ));
}

// 이것을 실패시키는 것: 빈·부분 쪽 속성의 기본값 경고 삭제, 완전한 속성에도 경고, 빈 마지막 구역 누락.
#[test]
fn missing_page_settings_warn_per_section_and_keep_core_defaults() {
    for (body, expected) in [
        ("<w:p/>", vec![("page_size", 1), ("page_margins", 1)]),
        ("<w:sectPr><w:pgMar w:left='100'/></w:sectPr>", vec![("page_size", 1), ("page_margins", 1)]),
        ("<w:sectPr><w:pgSz w:w='11906' w:h='16838'/></w:sectPr>", vec![("page_margins", 1)]),
        ("<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar/></w:sectPr>", vec![("page_margins", 1)]),
        ("<w:sectPr><w:pgSz/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>", vec![("page_size", 1)]),
        ("<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='0' w:right='0' w:bottom='0' w:left='0' w:header='0' w:footer='0' w:gutter='0'/></w:sectPr>", vec![]),
        ("<w:p><w:pPr><w:sectPr/></w:pPr></w:p>", vec![("page_size", 2), ("page_margins", 2)]),
    ] {
        let d = read(body);
        let warnings: Vec<_> = d
            .warnings
            .iter()
            .filter(|w| matches!(w.loss(), "page_size" | "page_margins"))
            .map(|w| {
                assert_eq!(
                    (w.code(), w.action()),
                    ("DOCX_PAGE_LAYOUT_APPROXIMATED", "use_defaults")
                );
                (w.loss(), w.count())
            })
            .collect();
        assert_eq!(warnings, expected, "{body}");
    }
    let d = read("<w:p/>");
    let p = d.document.sections()[0].page_settings;
    assert_eq!((p.width.as_i32(), p.height.as_i32(), p.landscape), (59528, 84189, false));
    assert_eq!(
        [
            p.margin_top,
            p.margin_right,
            p.margin_bottom,
            p.margin_left,
            p.header_margin,
            p.footer_margin,
            p.gutter
        ]
        .map(|v| v.as_i32()),
        [5669, 5669, 5669, 5669, 2835, 2835, 0]
    );
    let d = read("<w:p><w:pPr><w:sectPr/></w:pPr></w:p>");
    assert_eq!(texts(&d), [vec![""], vec![""]]);
}

// 이것을 실패시키는 것: orient 생략 세로에 추론 경고, 생략 가로 추론 경고 삭제.
#[test]
fn omitted_orientation_defaults_to_portrait_for_portrait_dimensions() {
    for (dimensions, expected) in [("w:w='11906' w:h='16838'", 0), ("w:w='16838' w:h='11906'", 1)] {
        let d = read(&format!("<w:sectPr><w:pgSz {dimensions}/></w:sectPr>"));
        assert_eq!(losses(&d, "page_orientation"), expected);
    }
}

// 이것을 실패시키는 것: 필수 속성 하나의 누락을 무시하거나 부분 여백의 명시값을 기본값으로 덮어씀.
#[test]
fn every_missing_page_attribute_warns_once_and_keeps_supplied_values() {
    for size in ["w:w='11906'", "w:h='16838'"] {
        let d = read(&format!("<w:sectPr><w:pgSz {size}/></w:sectPr>"));
        assert_eq!(losses(&d, "page_size"), 1);
    }
    for margins in [
        "w:left='100'",
        "w:right='0' w:bottom='0' w:left='100' w:header='0' w:footer='0' w:gutter='0'",
        "w:top='0' w:bottom='0' w:left='100' w:header='0' w:footer='0' w:gutter='0'",
        "w:top='0' w:right='0' w:left='100' w:header='0' w:footer='0' w:gutter='0'",
        "w:top='0' w:right='0' w:bottom='0' w:header='0' w:footer='0' w:gutter='0'",
        "w:top='0' w:right='0' w:bottom='0' w:left='100' w:footer='0' w:gutter='0'",
        "w:top='0' w:right='0' w:bottom='0' w:left='100' w:header='0' w:gutter='0'",
        "w:top='0' w:right='0' w:bottom='0' w:left='100' w:header='0' w:footer='0'",
    ] {
        let d = read(&format!(
            "<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar {margins}/></w:sectPr>"
        ));
        assert_eq!(losses(&d, "page_size"), 0);
        assert_eq!(losses(&d, "page_margins"), 1, "{margins}");
    }
    let d = read("<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:left='100'/></w:sectPr>");
    assert_eq!(d.document.sections()[0].page_settings.margin_left.as_i32(), 500);
}

// 이것을 실패시키는 것: 속성마다 경고 중복 집계, 요소 위치에서 경고하거나 loss/action/첫 구역 위치 변경.
#[test]
fn partial_page_settings_aggregate_at_section_boundaries() {
    let d = read("<w:p><w:pPr><w:sectPr><w:pgSz/><w:pgMar/></w:sectPr></w:pPr></w:p><w:sectPr><w:pgSz w:w='11906'/><w:pgMar w:left='100'/></w:sectPr>");
    let warnings: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| matches!(w.loss(), "page_size" | "page_margins"))
        .map(|w| (w.code(), w.loss(), w.action(), w.part(), w.location(), w.count()))
        .collect();
    assert_eq!(
        warnings,
        [
            (
                "DOCX_PAGE_LAYOUT_APPROXIMATED",
                "page_size",
                "use_defaults",
                Some("text/body.xml"),
                Some("sectPr#5"),
                2
            ),
            (
                "DOCX_PAGE_LAYOUT_APPROXIMATED",
                "page_margins",
                "use_defaults",
                Some("text/body.xml"),
                Some("sectPr#5"),
                2
            ),
        ]
    );
}

// 이것을 실패시키는 것: 전체 visible로 안쪽 결과 갱신 차단, 명령 경계 너머 결과 표시, 숨긴 나누기 출력.
#[test]
fn nested_results_inside_outer_instructions_are_present_but_do_not_split() {
    for result in ["<w:br w:type='page'/>", "<w:br w:type='column'/>", "<w:t>HIDDEN</w:t>"] {
        // The middle instruction also shields an outer result field from this result.
        let d = read(&format!("<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/>{result}<w:fldChar w:fldCharType='end'/><w:fldChar w:fldCharType='separate'/><w:fldChar w:fldCharType='end'/><w:fldChar w:fldCharType='end'/></w:r></w:p>"));
        assert_eq!(texts(&d), [vec![""]]);
        assert_eq!(losses(&d, "field_result"), 2);
        for (outer_result, expected_text, missing) in [("<w:t>7</w:t>", "7", 0), ("", "", 1)] {
            let d = read(&format!("<w:p><w:r><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='begin'/><w:fldChar w:fldCharType='separate'/>{result}<w:fldChar w:fldCharType='end'/><w:fldChar w:fldCharType='separate'/>{outer_result}<w:fldChar w:fldCharType='end'/></w:r></w:p>"));
            assert_eq!(texts(&d), [vec![expected_text]]);
            assert_eq!(
                d.document.sections()[0]
                    .paragraphs
                    .iter()
                    .map(|p| (p.page_break, p.column_break))
                    .collect::<Vec<_>>(),
                [(false, false)]
            );
            assert_eq!(losses(&d, "field_result"), missing);
            assert_eq!(losses(&d, "paragraph_split"), 0);
            assert_eq!(losses(&d, "column_break"), 0);
        }
    }
}

// 이것을 실패시키는 것: Word 거리를 Core에 그대로 복사, 영역 높이를 본문 높이에서 빼지 않음.
#[test]
fn word_report_margins_preserve_body_extents() {
    let body = "<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='1701' w:right='1440' w:bottom='1440' w:left='1440' w:header='851' w:footer='992' w:gutter='0'/></w:sectPr>";
    let d = read(body);
    let p = d.document.sections()[0].page_settings;
    assert_eq!(
        [p.margin_top, p.header_margin, p.margin_bottom, p.footer_margin].map(|n| n.as_i32()),
        [4255, 4250, 4960, 2240]
    );
    assert_eq!((p.margin_top + p.header_margin).as_i32(), 8505);
    assert_eq!((p.margin_bottom + p.footer_margin).as_i32(), 7200);
    assert_eq!([p.margin_left, p.margin_right, p.gutter].map(|n| n.as_i32()), [7200, 7200, 0]);
    assert_eq!(indexed(body).0[0].content_height.as_i32(), 68485);
    assert_eq!(
        losses(&d, "header_distance") + losses(&d, "footer_distance") + losses(&d, "page_margins"),
        0
    );
}

// 이것을 실패시키는 것: 음수 차 그대로 저장, 원래 본문 거리 변경, 경고 누락·중복·분류 변경, 같은 거리도 근사.
#[test]
fn overlapping_header_footer_distances_clamp_once_per_section() {
    for (top, header, bottom, footer, expected, warnings) in [
        (500, 720, 800, 200, [2500, 0, 1000, 3000], vec![("header_distance", 1)]),
        (800, 200, 500, 720, [1000, 3000, 2500, 0], vec![("footer_distance", 1)]),
        (
            500,
            720,
            500,
            720,
            [2500, 0, 2500, 0],
            vec![("header_distance", 1), ("footer_distance", 1)],
        ),
        (500, 500, 500, 500, [2500, 0, 2500, 0], vec![]),
    ] {
        let layout = format!("<w:sectPr><w:pgSz w:w='11906' w:h='16838'/><w:pgMar w:top='{top}' w:header='{header}' w:bottom='{bottom}' w:footer='{footer}' w:left='0' w:right='0' w:gutter='0'/></w:sectPr>");
        let d = read(&layout);
        let p = d.document.sections()[0].page_settings;
        assert_eq!(
            [p.margin_top, p.header_margin, p.margin_bottom, p.footer_margin].map(|n| n.as_i32()),
            expected
        );
        let actual: Vec<_> = d
            .warnings
            .iter()
            .filter(|w| matches!(w.loss(), "header_distance" | "footer_distance"))
            .map(|w| {
                assert_eq!(
                    (w.code(), w.action()),
                    ("DOCX_PAGE_LAYOUT_APPROXIMATED", "approximate")
                );
                (w.loss(), w.count())
            })
            .collect();
        assert_eq!(actual, warnings);
        let d = read(&format!("<w:p><w:pPr>{layout}</w:pPr></w:p>{layout}"));
        for (loss, _) in warnings {
            assert_eq!(losses(&d, loss), 2);
        }
    }
}

// 이것을 실패시키는 것: 누락 기본값 오해·명시값 덮어쓰기·본문 거리 변경·누락 header/footer 이중 근사 경고.
#[test]
fn missing_vertical_attributes_use_core_default_meanings_individually() {
    for (attrs, expected, distances) in [
        ("", [5669, 2835, 5669, 2835], [8504, 8504]),
        ("w:top='2000' w:bottom='2000'", [5669, 4331, 5669, 4331], [10000, 10000]),
        ("w:header='720' w:footer='720'", [3600, 4904, 3600, 4904], [8504, 8504]),
        ("w:top='500' w:bottom='500'", [2500, 0, 2500, 0], [2500, 2500]),
    ] {
        let d = read(&format!("<w:sectPr><w:pgMar {attrs}/></w:sectPr>"));
        let p = d.document.sections()[0].page_settings;
        assert_eq!(
            [p.margin_top, p.header_margin, p.margin_bottom, p.footer_margin].map(|n| n.as_i32()),
            expected
        );
        assert_eq!(
            [
                (p.margin_top + p.header_margin).as_i32(),
                (p.margin_bottom + p.footer_margin).as_i32()
            ],
            distances
        );
        assert_eq!(losses(&d, "header_distance") + losses(&d, "footer_distance"), 0);
        assert_eq!(losses(&d, "page_margins"), 1);
    }
}

// 이것을 실패시키는 것: 대응 뒤 바깥 여백만으로 본문 높이 검사, 0 허용, 음수 top/bottom 수용.
#[test]
fn mapped_body_height_rejects_zero_and_negative_but_accepts_one() {
    for (top, bottom, height, valid) in [
        (500, 500, "1000", false),
        (501, 500, "1000", false),
        (500, 500, "50.01pt", true),
        (-1, 500, "1000", false),
        (500, -1, "1000", false),
    ] {
        let body = format!("<w:sectPr><w:pgSz w:w='11906' w:h='{height}'/><w:pgMar w:top='{top}' w:bottom='{bottom}' w:header='100' w:footer='100'/></w:sectPr>");
        assert_eq!(decode_docx(&package(&body, &[])).is_ok(), valid, "{body}");
        if valid {
            assert_eq!(indexed(&body).0[0].content_height.as_i32(), 1);
        }
    }
}

// 이것을 실패시키는 것: 후행 빈 조각 유지, 나누기 전달 누락, 선행·중간 조각 제거, run 경계에 따른 차이, split 경고 중복.
#[test]
fn trailing_break_moves_only_to_next_output_paragraph() {
    for kind in ["page", "column"] {
        for (content, expected, breaks) in [
            (format!("<w:r><w:t>A</w:t><w:br w:type='{kind}'/><w:t>B</w:t><w:br w:type='{kind}'/></w:r>"), vec!["A", "B", "C"], vec![false, true, true]),
            (format!("<w:r><w:t>A</w:t></w:r><w:r><w:br w:type='{kind}'/></w:r><w:r><w:t>B</w:t></w:r><w:r><w:br w:type='{kind}'/></w:r><w:r><w:t/></w:r>"), vec!["A", "B", "C"], vec![false, true, true]),
            (format!("<w:r><w:br w:type='{kind}'/><w:br w:type='{kind}'/></w:r>"), vec!["", "", "C"], vec![false, true, true]),
        ] {
            for before in ["", "<w:pageBreakBefore/>"] {
                let d = read(&format!("<w:p>{content}</w:p><w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl><w:p><w:pPr>{before}</w:pPr><w:r><w:t>C</w:t></w:r></w:p>"));
                assert_eq!(texts(&d).as_slice(), std::slice::from_ref(&expected));
                assert_eq!(d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(), breaks);
                assert_eq!(losses(&d, "paragraph_split"), 1);
            }
        }
    }
    let d = read("<w:p><w:r><w:t>다</w:t></w:r></w:p><w:p><w:r><w:br w:type='page'/></w:r></w:p><w:p><w:r><w:t>둘째</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["다", "", "둘째"]]);
    assert_eq!(
        d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
        [false, false, true]
    );
    let d = read("<w:p><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p><w:p/><w:p><w:r><w:t>C</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["A", "", "C"]]);
    assert_eq!(
        d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
        [false, true, false]
    );
}

// 이것을 실패시키는 것: 본문 run이 있는 후행 조각 삭제, 구역 끝 조각 삭제, 다음 구역에 pending 나누기 유출.
#[test]
fn visible_tail_and_section_end_keep_their_fragments() {
    for (tail, text) in [
        ("<w:t>X</w:t>", "X"),
        ("<w:tab/>", "\t"),
        ("<w:br/>", "\n"),
        ("<w:t xml:space='preserve'> </w:t>", " "),
    ] {
        let d = read(&format!("<w:p><w:r><w:t>A</w:t><w:br w:type='page'/>{tail}</w:r></w:p><w:p><w:r><w:t>C</w:t></w:r></w:p>"));
        assert_eq!(texts(&d), [vec!["A", text, "C"]]);
        assert_eq!(
            d.document.sections()[0].paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(),
            [false, true, false]
        );
    }
    let d = read("<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/><w:t>B</w:t><w:br w:type='page'/></w:r></w:p><w:p><w:r><w:t>C</w:t></w:r></w:p>");
    assert_eq!(texts(&d), [vec!["A", "B", ""], vec!["C"]]);
    assert_eq!(
        d.document
            .sections()
            .iter()
            .map(|s| s.paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [vec![false, true, true], vec![false]]
    );
}

// 이것을 실패시키는 것: 뒤 간격·keepNext·끝 모양을 삭제된 조각에 남김, 첫 조각 속성 삭제·마지막 조각에 복제, 여러 원본의 pending 덮어쓰기.
#[test]
fn moved_tail_restores_last_fragment_properties_and_origins() {
    for prefix in ["", "<w:t>A</w:t><w:br w:type='page'/>"] {
        let source = format!("<w:p><w:pPr><w:spacing w:before='20' w:after='40'/><w:ind w:firstLine='10'/><w:keepNext/><w:rPr><w:color w:val='123456'/></w:rPr></w:pPr><w:r>{prefix}<w:t>B</w:t><w:br w:type='page'/></w:r></w:p>");
        let body = format!("{source}{source}<w:p><w:r><w:t>C</w:t></w:r></w:p>");
        let d = read(&body);
        let p = &d.document.sections()[0].paragraphs;
        let n = if prefix.is_empty() { 1 } else { 2 };
        assert_eq!(p.len(), n * 2 + 1);
        for start in [0, n] {
            let first = &d.registry.para_shapes[p[start].para_shape_id.get()];
            let last = &d.registry.para_shapes[p[start + n - 1].para_shape_id.get()];
            assert_eq!((first.space_before.as_i32(), first.indent_first_line.as_i32()), (100, 50));
            assert_eq!((last.space_after.as_i32(), last.keep_with_next), (200, true));
            assert_eq!(
                (last.space_before.as_i32(), last.indent_first_line.as_i32()),
                if n == 1 { (100, 50) } else { (0, 0) }
            );
            let mark = p[start + n - 1].end_char_shape.unwrap();
            assert_eq!(
                d.registry.char_shapes[mark.get()].color,
                hwpforge_foundation::Color::from_rgb(0x12, 0x34, 0x56)
            );
        }
        assert_eq!(losses(&d, "paragraph_split"), if prefix.is_empty() { 0 } else { 2 });
    }
    let s = indexed(
        "<w:p><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>",
    )
    .0;
    assert_eq!(
        s[0].origins.iter().map(|o| (o.paragraph, o.fragment, o.cause)).collect::<Vec<_>>(),
        [(1, 0, None), (2, 0, None)]
    );
}

// 이것을 실패시키는 것: 한 조각에도 fragment_shape 적용, keepLines·widow·간격 손실, 분할 경고 유지.
#[test]
fn moved_single_fragment_preserves_original_shape_without_split_warning() {
    let d = read("<w:p><w:pPr><w:keepLines/><w:widowControl/><w:spacing w:before='20' w:after='40'/></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p><w:p><w:r><w:t>B</w:t></w:r></w:p>");
    let paragraphs = &d.document.sections()[0].paragraphs;
    assert_eq!(texts(&d), [vec!["A", "B"]]);
    let shape = &d.registry.para_shapes[paragraphs[0].para_shape_id.get()];
    assert!(shape.keep_lines_together);
    assert!(shape.widow_orphan);
    assert_eq!((shape.space_before.as_i32(), shape.space_after.as_i32()), (100, 200));
    assert_eq!(paragraphs.iter().map(|p| p.page_break).collect::<Vec<_>>(), [false, true]);
    assert_eq!(losses(&d, "paragraph_split"), 0);
}

// 이것을 실패시키는 것: 구역 끝에서 버릴 마지막 조각 복구 모양을 미리 intern, 끝 빈 조각·분할 경고 삭제.
#[test]
fn section_end_does_not_intern_unused_tail_restoration() {
    for (body, expected_texts, shape_count) in [
        ("<w:p><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p>", vec!["A", ""], 1),
        ("<w:p><w:pPr><w:keepLines/><w:spacing w:before='20' w:after='40'/></w:pPr><w:r><w:t>base</w:t></w:r></w:p><w:p><w:pPr><w:keepLines/><w:spacing w:before='20' w:after='40'/></w:pPr><w:r><w:t>A</w:t><w:br w:type='page'/></w:r></w:p>", vec!["base", "A", ""], 3),
    ] {
        let d = read(body);
        let paragraphs = &d.document.sections()[0].paragraphs;
        assert_eq!(texts(&d), [expected_texts]);
        assert_eq!(d.registry.para_shapes.len(), shape_count);
        let used: std::collections::HashSet<_> = paragraphs.iter().map(|p| p.para_shape_id.get()).collect();
        assert_eq!(used.len(), shape_count);
        assert_eq!(losses(&d, "paragraph_split"), 1);
    }
}
