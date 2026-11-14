use super::*;

fn plain(id: usize, text: &str) -> String {
    format!(
        r#"<hp:p id="{id}" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>{text}</hp:t></hp:run>{CACHE}</hp:p>"#
    )
}

fn refuse(base: &[u8]) {
    assert!(delete_layout::check(&RawPackage::read(base).unwrap(), &xml(base)).is_err());
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(base, &[TARGET]).unwrap_err(),
        original_error(base)
    );
}
fn exact_delete(base: &[u8], target: &str, index: usize) {
    let original = xml(base);
    assert_eq!(original.matches(target).count(), 1);
    let result =
        HwpxStructuralEditor::delete_paragraphs(base, &[ParagraphLocator { section: 0, index }])
            .unwrap();
    assert_eq!(xml(&result), original.replacen(target, "", 1));
}

// 이것을 실패시키는 것: 보존 삭제의 뒤 캐시 제거 또는 ID 재배정 적용.
#[test]
fn source_deletion_matches_independent_byte_oracle() {
    let base = table_base();
    let raw = xml(&base);
    let d = HwpxDecoder::decode(&base).unwrap();
    let first = &raw[d.source_maps[0].paragraphs[0].outer.clone()];
    let table = &raw[d.source_maps[0].paragraphs[2].outer.clone()];
    let target = plain(817, "대상");
    for (body, index) in [
        (format!("{first}{target}{}{table}{}", plain(901, "중간"), plain(456, "끝")), 1),
        (format!("{first}{}{target}{table}{}", plain(901, "앞"), plain(456, "끝")), 2),
        (format!("{first}{}{table}{target}", plain(901, "앞")), 3),
    ] {
        let header_end = raw.find(first).unwrap();
        let original = format!("{}{body}<!--keep--> \n</hs:sec>", &raw[..header_end]);
        let original = original.replace("height=\"321\"", "height=\"1234\"");
        let base = replace(&base, SECTION, &raw, &original);
        exact_delete(&base, &target, index);
    }
}

// 이것을 실패시키는 것: 표 높이 0 치환 또는 표 모양 허용 목록 복원.
#[test]
fn source_deletion_preserves_unrestricted_table_bytes() {
    use hwpforge_core::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    let rows = (0..2)
        .map(|_| {
            TableRow::new(
                (0..7)
                    .map(|_| {
                        TableCell::new(
                            vec![paragraph("칸"), table_host()],
                            HwpUnit::new(9000).unwrap(),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    let mut host = paragraph("표");
    host.runs.push(Run::table(Table::new(rows), CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("처음"), paragraph("삭제"), host],
        PageSettings::a4(),
    ));
    let encoded = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = extra(&encoded, "Scripts/opaque.bin", b"carry");
    let raw = xml(&base).replace("height=\"0\" heightRelTo=", "height=\"1234\" heightRelTo=");
    let base = replace(&base, SECTION, &xml(&base), &raw);
    let mut raw = xml(&base);
    let margin = raw.find("<hp:cellMargin ").unwrap();
    let left = margin + raw[margin..].find("left=\"").unwrap() + 6;
    let end = left + raw[left..].find('"').unwrap();
    raw.replace_range(left..end, "123");
    let base = replace(&base, SECTION, &xml(&base), &raw);
    let raw = xml(&base);
    let d = HwpxDecoder::decode(&base).unwrap();
    exact_delete(&base, &raw[d.source_maps[0].paragraphs[1].outer.clone()], 1);
}

// 이것을 실패시키는 것: Reindex의 캐시 제거/ID 재배정 생략 또는 새 검사를 일반 경로에도 적용.
#[test]
fn admitted_delete_and_insert_keep_original_splice_behavior() {
    let base = generated();
    let original = xml(&base);
    let d = HwpxDecoder::decode(&base).unwrap();
    let tail = &original[d.source_maps[0].paragraphs[2].outer.clone()];
    let base = replace(
        &base,
        SECTION,
        tail,
        &tail.replace("id=\"2\"", "id=\"999\"").replace("</hp:p>", &format!("{CACHE}</hp:p>")),
    );
    assert!(admit(&base).is_ok());
    failures(&[]);
    let output = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    assert!(!xml(&output).contains(CACHE));
    assert!(!xml(&output).contains("id=\"999\""));
    assert!(xml(&output).contains("id=\"1\""));
    assert!(!visits().contains(&"derived-heading"));
    assert!(!visits().contains(&"delete-layout"));
    let inserted = HwpxStructuralEditor::insert_paragraphs(
        &base,
        TARGET,
        InsertPosition::After,
        &["추가".into()],
    )
    .unwrap();
    let input = xml(&base);
    let d = HwpxDecoder::decode(&base).unwrap();
    let inserted_xml = xml(&inserted);
    // 이것을 실패시키는 것: 삽입 fragment 뒤에 의미 없는 주석 추가.
    let anchor = &input[d.source_maps[0].paragraphs[1].outer.clone()];
    let added = anchor.replace("id=\"1\"", "id=\"2\"").replace("가 &amp; &lt;나&gt;끝", "추가");
    let tail = &input[d.source_maps[0].paragraphs[2].outer.clone()];
    let expected_tail = tail.replace("id=\"999\"", "id=\"3\"").replace(CACHE, "");
    let expected = input.replacen(tail, &(added + &expected_tail), 1);
    assert_eq!(inserted_xml, expected);
}

// 이것을 실패시키는 것: 루트/이름공간/자식 목록/pos 소유/단/방향/설정 개수 검사 중 하나 생략.
#[test]
fn layout_rejections_return_original_admission_error() {
    let base = carry_base();
    for (from, to) in [
        ("</hs:sec>", "<hp:p><hp:run><hp:unknown/></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:run><x:tbl xmlns:x='urn:other'/></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:run><hp:ctrl><hp:t/></hp:ctrl></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:pos treatAsChar='1'/></hp:p></hs:sec>"),
        ("colCount=\"1\"", "colCount=\"2\""),
        ("textDirection=\"HORIZONTAL\"", "textDirection=\"VERTICAL\""),
        ("</hs:sec>", "<hp:p><hp:run><hp:secPr textDirection='HORIZONTAL'/></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:run><hp:secPr textDirection='HORIZONTAL'><hp:pagePr/></hp:secPr></hp:run></hp:p></hs:sec>"),
    ] { refuse(&replace(&base, SECTION, from, to)); }
    refuse(&replace(
        &base,
        SECTION,
        "</hs:sec>",
        "<hp:p><x:tbl xmlns:x='urn:other'/></hp:p></hs:sec>",
    ));
    let master = "<wrong xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph'/>";
    refuse(&extra(&base, "Contents/masterpage0.xml", master.as_bytes()));
}

// 이것을 실패시키는 것: pos 없는 개체를 인라인으로 취급, 표만 검사, 삭제 뒤만 검사 또는 배치 검사 생략.
#[test]
fn wrapping_objects_are_checked_in_every_flow() {
    let base = table_base();
    for source in [
        "<hp:tbl textWrap='SQUARE'><hp:pos treatAsChar='0'/></hp:tbl>",
        "<hp:tbl><hp:pos treatAsChar='1'/></hp:tbl>",
        "<hp:tbl textWrap='TOP_AND_BOTTOM'><hp:pos treatAsChar='1'/><hp:pos treatAsChar='1'/></hp:tbl>",
        "<hp:tbl textWrap='SQUARE'/>",
        "<hp:tbl textWrap='TOP_AND_BOTTOM'><hp:tr><hp:tc><hp:subList><hp:p><hp:run><hp:rect textWrap='TIGHT'><hp:pos treatAsChar='0'/></hp:rect></hp:run></hp:p></hp:subList></hp:tc></hp:tr></hp:tbl>",
        "<hp:ctrl><hp:header><hp:subList><hp:p><hp:run><hp:pic textWrap='SQUARE'><hp:pos treatAsChar='0'/></hp:pic></hp:run></hp:p></hp:subList></hp:header></hp:ctrl>",
    ] {
        let input = replace(&base, SECTION, "</hp:run>", &format!("{source}</hp:run>"));
        refuse(&input);
    }
}

// 이것을 실패시키는 것: TOP_AND_BOTTOM만 허용하도록 줄이거나 세로 이동에만 영향 주는 구조 거부.
#[test]
fn vertical_only_layout_changes_preserve_section_bytes() {
    use hwpforge_core::{control::InlinePageKind, section::HeaderFooter, Control};
    let mut section = Section::with_paragraphs(
        vec![paragraph("처음"), paragraph("삭제"), table_host(), paragraph("끝")],
        PageSettings::a4(),
    );
    let number = Paragraph::with_runs(
        vec![Run::control(
            Control::InlinePageNumber { kind: InlinePageKind::CurrentPage },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    section.footers.push(HeaderFooter::all_pages(vec![number]));
    section.paragraphs[3].runs.extend(table_host().runs.clone());
    section.paragraphs[3]
        .runs
        .push(Run::control(Control::footnote(vec![paragraph("각주")]), CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(section);
    let encoded = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = extra(&encoded, "Scripts/opaque.bin", b"carry");
    let original = xml(&base);
    let d = HwpxDecoder::decode(&base).unwrap();
    let target = original[d.source_maps[0].paragraphs[1].outer.clone()].to_owned();
    let base = replace(&base, SECTION, "treatAsChar=\"0\"", "treatAsChar=\"1\"");
    let base = replace(&base, SECTION, "textWrap=\"TOP_AND_BOTTOM\"", "textWrap=\"SQUARE\"");
    let inline_pic = super::losses::picture("1", "<hp:pos treatAsChar='1'/>")
        .replace("textWrap=\"TOP_AND_BOTTOM\"", "textWrap=\"SQUARE\"");
    let front_pic =
        super::losses::picture("1", "<hp:pos treatAsChar='0' vertRelTo='PAGE' horzRelTo='PAPER'/>")
            .replace("textWrap=\"TOP_AND_BOTTOM\"", "textWrap=\"IN_FRONT_OF_TEXT\"");
    let rect = super::losses::object("rect", 1000, 1000, "1", "<hp:pos treatAsChar='0' allowOverlap='1'/><hp:caption><hp:subList><hp:p><hp:run><hp:t>캡션</hp:t></hp:run></hp:p></hp:subList></hp:caption>").replace("textWrap=\"TOP_AND_BOTTOM\"", "textWrap=\"BEHIND_TEXT\"");
    let group =
        super::losses::object("container", 1000, 1000, "1", &super::losses::picture("1", ""));
    let extra_objects = format!("{inline_pic}{front_pic}{rect}{group}");
    let base =
        replace(&base, SECTION, "<hp:t>끝</hp:t>", &format!("<hp:t>끝</hp:t>{extra_objects}"));
    for page_break in ["CELL", "TABLE", "NONE"] {
        let base =
            replace(&base, SECTION, "pageBreak=\"CELL\"", &format!("pageBreak=\"{page_break}\""));

        let current = xml(&base);
        let d = HwpxDecoder::decode(&base).unwrap();
        let tail = &current[d.source_maps[0].paragraphs[3].outer.clone()];
        let base =
            replace(&base, SECTION, tail, &tail.replacen("pageBreak=\"0\"", "pageBreak=\"1\"", 1));
        let base =
            replace(&base, "Contents/header.xml", "keepWithNext=\"0\"", "keepWithNext=\"1\"");
        exact_delete(&base, &target, 1);
    }
}

// 이것을 실패시키는 것: 정상 원문 삭제에서 직접/스타일 heading 검사 호출 제거.
#[test]
fn plain_deleted_headings_retain_original_error() {
    let base = carry_base();
    for inherited in [false, true] {
        for kind in ["NUMBER", "OUTLINE"] {
            let input = super::losses::outline_change_heading(&base, inherited, kind, 0, 1);
            assert_eq!(
                HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
                original_error(&input)
            );
        }
    }
}

// 이것을 실패시키는 것: 배치 검사를 편집 구역에만 한정하거나 바탕쪽 경로의 대소문자를 구별.
#[test]
fn layout_scope_includes_all_master_pages_only() {
    let base = carry_base();
    let section = xml(&base);
    let hp = "http://www.hancom.co.kr/hwpml/2011/paragraph";
    let good = format!("<masterPage xmlns:hp='{hp}'><hp:p><hp:run><hp:rect textWrap='TOP_AND_BOTTOM'/></hp:run></hp:p></masterPage>");
    for root in [
        good.clone(),
        good.replace(
            "<masterPage ",
            "<masterPage xmlns='http://www.hancom.co.kr/hwpml/2011/master-page' ",
        ),
    ] {
        let input = extra(&base, "Contents/MASTERPAGE-unused.XML", root.as_bytes());
        assert!(delete_layout::check(&RawPackage::read(&input).unwrap(), &section).is_ok());
    }
    let bad = good.replace("TOP_AND_BOTTOM", "SQUARE");
    let input = extra(&base, "Contents/MASTERPAGE-unused.XML", bad.as_bytes());
    assert!(delete_layout::check(&RawPackage::read(&input).unwrap(), &section).is_err());
    for path in ["Contents/header-extra.xml", "Contents/section99.xml"] {
        let input = extra(&base, path, bad.as_bytes());
        assert!(delete_layout::check(&RawPackage::read(&input).unwrap(), &section).is_ok());
    }
}

// 이것을 실패시키는 것: DTD/속성 오류/파싱 오류 승인, 개체 증거 또는 구역/쪽 설정 개수 검사 생략.
#[test]
fn layout_parser_requires_explicit_valid_evidence() {
    let base = carry_base();
    let package = RawPackage::read(&base).unwrap();
    let original = xml(&base);
    for (from, to) in [
        ("</hs:sec>", "<hp:p/><!DOCTYPE x></hs:sec>"),
        ("</hs:sec>", "<hp:p id='a' id='b'/></hs:sec>"),
        ("</hs:sec>", "<hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:run><hp:tbl textWrap='UNKNOWN'/></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p><hp:run><hp:tbl textWrap='TOP_AND_BOTTOM'><hp:pos treatAsChar='2'/></hp:tbl></hp:run></hp:p></hs:sec>"),
        ("</hs:sec>", "<hp:p textWrap='TOP_AND_BOTTOM'/></hs:sec>"),
        ("</hp:secPr>", "<hp:pagePr/></hp:secPr>"),
        ("colCount=\"1\"", "colCountMissing=\"1\""),
    ] {
        assert!(original.contains(from));
        assert!(delete_layout::check(&package, &original.replacen(from, to, 1)).is_err(), "{to}");
    }
    let alias = original.replace("hp:", "q:").replace("xmlns:hp=", "xmlns:q=");
    assert!(delete_layout::check(&package, &alias).is_ok());
    let normalized = original.replace("colCount=\"1\"", "colCount=\"&#49;\"");
    assert!(delete_layout::check(&package, &normalized).is_ok());
}

// 이것을 실패시키는 것: 이름공간 선언 수 상한 생략·조정, 요소 깊이 상한 생략·조정.
#[test]
fn layout_parser_bounds_declarations_and_depth() {
    let base = carry_base();
    let package = RawPackage::read(&base).unwrap();
    let original = xml(&base);
    let existing = original.matches(" xmlns").count();
    let declare = |total: usize| {
        let extra: String = (existing..total).map(|i| format!(" xmlns:x{i}='urn:x{i}'")).collect();
        original.replacen("<hs:sec", &format!("<hs:sec{extra}"), 1)
    };
    assert!(delete_layout::check(&package, &declare(64)).is_ok());
    assert!(delete_layout::check(&package, &declare(65)).is_err());
    // The section root and a paragraph sit above the nested chain.
    let nest = |depth: usize| {
        let chain = format!("{}{}", "<hp:x>".repeat(depth - 2), "</hp:x>".repeat(depth - 2));
        original.replacen("</hs:sec>", &format!("<hp:p>{chain}</hp:p></hs:sec>"), 1)
    };
    let limit = crate::decoder::XML_RECURSION_LIMIT;
    assert!(delete_layout::check(&package, &nest(limit)).is_ok());
    assert!(delete_layout::check(&package, &nest(limit + 1)).is_err());
}

// 이것을 실패시키는 것: 삭제와 무관한 정적 정의에 전역 요소 이름 목록 적용, 쪽 번호의 pos 속성을 개체 위치로 오인.
#[test]
fn static_definitions_survive_source_deletion() {
    let base = extra(
        &carry_base(),
        "BinData/pattern.png",
        include_bytes!("../../../../tests/fixtures/images/main-charactor.png"),
    );
    let base = replace(
        &base,
        "Contents/content.hpf",
        "</opf:manifest>",
        r#"<opf:item id="pattern" href="BinData/pattern.png" media-type="image/png" isEmbeded="1"/></opf:manifest>"#,
    );
    let static_content = format!(
        "{}<hp:ctrl><hp:pageNum pos='BOTTOM_CENTER' formatType='DIGIT' sideChar='-'/></hp:ctrl><hp:ctrl><hp:indexmark><hp:firstKey>primary</hp:firstKey><hp:secondKey>secondary</hp:secondKey></hp:indexmark></hp:ctrl><hp:ctrl><hp:fieldBegin id='73' type='HYPERLINK'><hp:parameters><hp:stringParam name='Command'>https://example.com;1;0;0;</hp:stringParam><hp:booleanParam name='enabled'>true</hp:booleanParam></hp:parameters></hp:fieldBegin></hp:ctrl><hp:t>link</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='73'/></hp:ctrl>",
        losses::object("curve", 1000, 1000, "1", "<hp:lineShape color='#000000' width='33' style='SOLID'/><hp:seg type='CURVE' x1='0' y1='0' x2='1000' y2='1000'/><hc:fillBrush><hc:imgBrush mode='TOTAL'><hc:img binaryItemIDRef='pattern' effect='REAL_PIC' bright='0' contrast='0' alpha='0'/></hc:imgBrush></hc:fillBrush><hp:effects><hp:glow/></hp:effects>")
    );
    let base = replace(&base, SECTION, "<hp:t>남음</hp:t>", &static_content);
    let raw = xml(&base);
    let decoded = HwpxDecoder::decode(&base).unwrap();
    exact_delete(&base, &raw[decoded.source_maps[0].paragraphs[1].outer.clone()], 1);
}

// 이것을 실패시키는 것: run과 대안 분기의 닫힌 자식 목록 검사 생략.
#[test]
fn unknown_run_children_are_rejected() {
    let base = carry_base();
    for child in [
        "<hp:unknown/>",
        "<hp:switch><hp:case><hp:unknown/></hp:case></hp:switch>",
        "<hp:switch><hp:default><hp:unknown/></hp:default></hp:switch>",
    ] {
        refuse(&replace(&base, SECTION, "</hp:run>", &format!("{child}</hp:run>")));
    }
}

// 이것을 실패시키는 것: textWrap 속성이나 hp:pos 요소를 가진 미지 요소 승인, pos 요소 이름공간 검사 생략.
#[test]
fn unknown_positioned_descendants_are_rejected() {
    let base = carry_base();
    for child in [
        "<hp:unknown><hp:pos treatAsChar='1'/></hp:unknown>",
        "<x:rect xmlns:x='urn:other' textWrap='TOP_AND_BOTTOM'><hp:pos treatAsChar='1'/></x:rect>",
        "<hp:unknown textWrap='TOP_AND_BOTTOM'/>",
        "<hp:rect textWrap='TOP_AND_BOTTOM'><hc:pos treatAsChar='1'/></hp:rect>",
        "<hp:rect textWrap='TOP_AND_BOTTOM'><x:pos xmlns:x='urn:other' treatAsChar='1'/></hp:rect>",
    ] {
        refuse(&replace(&base, SECTION, "</hp:t>", &format!("{child}</hp:t>")));
    }
}

// 이것을 실패시키는 것: container의 닫힌 자식 목록 검사 생략.
#[test]
fn unknown_container_children_are_rejected() {
    let base = carry_base();
    refuse(&replace(
        &base,
        SECTION,
        "</hp:run>",
        "<hp:container textWrap='TOP_AND_BOTTOM'><hp:unknown/></hp:container></hp:run>",
    ));
}
