use super::*;

fn character() -> Tag {
    let languages = |name, value| {
        format!("<{name} hangul='{value}' latin='{value}' hanja='{value}' japanese='{value}' other='{value}' symbol='{value}' user='{value}'/>")
    };
    Tag::read(&format!("<charPr height='1000' useFontSpace='0' useKerning='0' symMark='NONE'>{}{}{}{}<shadow type='NONE' offsetX='10' offsetY='10'/></charPr>", languages("ratio", 100), languages("spacing", 0), languages("relSz", 100), languages("offset", 0))).unwrap()
}

// 이것을 실패시키는 것: 7언어 동일성/존재/저장소 일치 검사 생략, offset 제외.
#[test]
fn language_metrics_require_all_seven_explicit_matching_values() {
    let shape = HwpxCharShape::default();
    assert!(character_allows(&character(), &shape).is_some());
    for name in ["ratio", "spacing", "relSz", "offset"] {
        let mut missing = character();
        missing.children.retain(|c| c.name != name);
        assert!(character_allows(&missing, &shape).is_none(), "missing {name}");
        for language in ["hangul", "latin", "hanja", "japanese", "other", "symbol", "user"] {
            for value in [None, Some("3"), Some("invalid")] {
                let mut tag = character();
                let child = tag.children.iter_mut().find(|c| c.name == name).unwrap();
                match value {
                    Some(value) => {
                        child.attrs.insert(language.into(), value.into());
                    }
                    None => {
                        child.attrs.remove(language);
                    }
                }
                assert!(character_allows(&tag, &shape).is_none(), "{name}/{language}/{value:?}");
            }
        }
        let mut tag = character();
        let child = tag.children.iter_mut().find(|c| c.name == name).unwrap();
        for value in child.attrs.values_mut() {
            *value = "3".into();
        }
        assert!(character_allows(&tag, &shape).is_none(), "store mismatch {name}");
        let mut shape = shape.clone();
        match name {
            "ratio" => shape.ratio = 3,
            "spacing" => shape.spacing = 3,
            "relSz" => shape.rel_sz = 3,
            _ => shape.char_offset = 3,
        }
        assert_eq!(character_allows(&tag, &shape).is_some(), name != "offset", "uniform {name}");
    }
}

// 이것을 실패시키는 것: 효과 하나의 검사 삭제, unknown을 NONE으로 처리, NONE shadow offset 거부.
#[test]
fn character_effects_and_font_spacing_need_inactive_evidence() {
    let shape = HwpxCharShape::default();
    for attribute in ["useFontSpace", "useKerning", "symMark"] {
        for value in [None, Some("1"), Some("UNKNOWN")] {
            let mut tag = character();
            match value {
                Some(value) => {
                    tag.attrs.insert(attribute.into(), value.into());
                }
                None => {
                    tag.attrs.remove(attribute);
                }
            }
            assert!(character_allows(&tag, &shape).is_none(), "{attribute}/{value:?}");
        }
    }
    for name in ["bold", "italic", "emboss", "engrave", "supscript", "subscript"] {
        let mut tag = character();
        tag.children.push(Tag::read(&format!("<{name}/>")).unwrap());
        assert!(character_allows(&tag, &shape).is_none(), "{name}");
    }
    for (name, key) in
        [("underline", "type"), ("strikeout", "shape"), ("outline", "type"), ("shadow", "type")]
    {
        for value in ["NONE", "SOLID", "UNKNOWN"] {
            let mut tag = character();
            tag.children.retain(|c| c.name != name);
            tag.children.push(Tag::read(&format!("<{name} {key}='{value}'/>")).unwrap());
            assert_eq!(character_allows(&tag, &shape).is_some(), value == "NONE", "{name}/{value}");
        }
    }
    assert!(character_allows(&character(), &shape).is_some());
    let mut duplicate = character();
    duplicate.children.push(Tag::read("<shadow type='NONE'/>").unwrap());
    assert!(character_allows(&duplicate, &shape).is_none());
    let mut omitted = character();
    omitted.children.retain(|c| c.name != "shadow");
    assert!(character_allows(&omitted, &shape).is_some());
}

// 이것을 실패시키는 것: 서로 다른 빈 run 모양 무시, 끝 표시 허용, 원본 폭 재검사.
#[test]
fn all_direct_runs_including_empty_runs_must_have_one_style() {
    use hwpforge_core::{
        layout::{LayoutCache, LineSeg},
        Run,
    };
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_char_shape(HwpxCharShape::default());
    let mut p = Paragraph::with_runs(
        vec![Run::text("가나", CharShapeIndex::new(0)), Run::text("", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut line = LineSeg::default();
    line.horzsize = 1050;
    p.layout_cache = Some(LayoutCache::new(vec![line]));
    assert!(eligible(&p, "가나", "가", &styles).is_some());
    p.runs[1].char_shape_id = CharShapeIndex::new(1);
    assert!(eligible(&p, "가나", "가", &styles).is_none());
    p.runs[1].char_shape_id = CharShapeIndex::new(0);
    p.end_char_shape = Some(CharShapeIndex::new(0));
    assert!(eligible(&p, "가나", "가", &styles).is_none());
}

fn wire_fixture() -> (RawPackage, crate::HwpxDocument) {
    use hwpforge_core::{Document, ImageStore, PageSettings, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, FontIndex, ParaShapeIndex};
    let mut styles = HwpxStyleStore::new();
    for language in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
        for index in 0..2 {
            let mut font =
                crate::style_store::HwpxFont::new(index, format!("{language}{index}"), language);
            font.substitute = Some(crate::style_store::HwpxSubstFont::new(
                Some(crate::style_store::HwpxFontType::Ttf),
                format!("Sub{language}{index}"),
            ));
            styles.push_font(font);
        }
    }
    let mut character = HwpxCharShape::default();
    character.font_ref.latin = FontIndex::new(1);
    character.font_ref.other = FontIndex::new(1);
    styles.push_char_shape(character);
    styles.push_para_shape(crate::style_store::HwpxParaShape::default());
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes =
        crate::HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    (RawPackage::read(&bytes).unwrap(), crate::HwpxDecoder::decode(&bytes).unwrap())
}

fn proof(package: &RawPackage, decoded: &crate::HwpxDocument, reencoded: bool) -> bool {
    let Some(header) = Header::read(package, &decoded.style_store, || {}) else {
        return false;
    };
    let section =
        SectionEvidence::read(&package.read_text_entry("Contents/section0.xml").unwrap()).unwrap();
    let source = references(
        "<hp:p paraPrIDRef='0' styleIDRef='0'>",
        ["<hp:run charPrIDRef='0'>"],
        &section.namespaces,
    )
    .unwrap();
    header.proves(
        &section,
        &source,
        &decoded.document.sections()[0].paragraphs[0],
        &decoded.style_store,
        reencoded,
    )
}

// 이것을 실패시키는 것: 공용 검사 무조건 true, height/offset 검사 생략,
// 누락된 원문 값을 Core 기본값으로 승인, 문단 변환 대조 검사 생략.
#[test]
fn explicit_definitions_must_match_the_values_used_by_the_proof() {
    let (mut package, decoded) = wire_fixture();
    assert!(proof(&package, &decoded, true));
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    for (from, to) in [
        ("height=\"1000\"", "height=\"900\""),
        ("height=\"1000\"", "height=\"0\""),
        ("height=\"1000\"", "height=\"2147483648\""),
        ("height=\"1000\"", ""),
        ("<hh:offset hangul=\"0\"", "<hh:offset hangul=\"1\""),
        ("snapToGrid=\"1\"", "snapToGrid=\"0\""),
        ("snapToGrid=\"1\"", ""),
        ("horizontal=\"LEFT\"", "horizontal=\"UNKNOWN\""),
        ("tabPrIDRef=\"0\"", "tabPrIDRef=\"1\""),
        ("keepWithNext=\"0\"", "keepWithNext=\"1\""),
        ("<hc:left value=\"0\"", "<hc:left value=\"1\""),
        (
            "required-namespace=\"http://www.hancom.co.kr/hwpml/2016/HwpUnitChar\"",
            "required-namespace=\"urn:unknown\"",
        ),
    ] {
        assert!(xml.contains(from), "fixture contains {from}");
        package.replace_text_entry(HEADER_PART, xml.replace(from, to));
        assert!(!proof(&package, &decoded, false), "{from} → {to}");
    }
}

// 이것을 실패시키는 것: fontRef 숫자만 비교, 언어군 평탄화, 글꼴 대조를 raw splice에도 적용,
// face 정리·type/substFont/typeInfo 부재를 기본값으로 덮어 승인.
#[test]
fn reencoded_fonts_resolve_each_language_position_and_metadata() {
    let (mut package, decoded) = wire_fixture();
    assert!(proof(&package, &decoded, true));
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    for (language, index) in [
        ("HANGUL", 0),
        ("LATIN", 1),
        ("HANJA", 0),
        ("JAPANESE", 0),
        ("OTHER", 1),
        ("SYMBOL", 0),
        ("USER", 0),
    ] {
        let start =
            xml.find(&format!("<hh:font id=\"{index}\" face=\"{language}{index}\"")).unwrap();
        let end = start + xml[start..].find("</hh:font>").unwrap() + "</hh:font>".len();
        let font = &xml[start..end];
        let info_start = font.find("<hh:typeInfo").unwrap();
        let info_end = info_start + font[info_start..].find("/>").unwrap() + 2;
        let sub_start = font.find("<hh:substFont").unwrap();
        let sub_end = sub_start + font[sub_start..].find("/>").unwrap() + 2;
        for changed in [
            font.replace(&format!("face=\"Sub{language}{index}\""), "face=\"Different\""),
            font.replace(&format!("face=\"Sub{language}{index}\" type=\"TTF\""), &format!("face=\"Sub{language}{index}\" type=\"HFT\"")),
            format!("{}{}", &font[..sub_start], &font[sub_end..]),
            font.replace(&format!("face=\"{language}{index}\""), "face=\"Other\""),
            font.replace(&format!("face=\"{language}{index}\""), &format!("face=\" {language}{index} \"")),
            font.replace("type=\"TTF\"", "type=\"HFT\""),
            font.replace("type=\"TTF\"", ""),
            font.replace("</hh:font>", "<hh:substFont face=\"Other\" type=\"TTF\" isEmbedded=\"0\" binaryItemIDRef=\"\"/></hh:font>"),
            font.replace("weight=\"6\"", "weight=\"7\""),
            format!("{}{}", &font[..info_start], &font[info_end..]),
            font.replacen("isEmbedded=\"0\"", "isEmbedded=\"1\"", 1),
            font.replace(&font[sub_start..sub_end], &font[sub_start..sub_end].replace("isEmbedded=\"0\"", "isEmbedded=\"1\"")),
            font.replace("familyType=\"FCAT_GOTHIC\"", "familyType=\"FCAT_MYUNGJO\""),
        ] {
            assert_ne!(font, changed);
            package.replace_text_entry(HEADER_PART, format!("{}{}{}", &xml[..start], changed, &xml[end..]));
            assert!(proof(&package, &decoded, false), "raw {language}");
            assert!(!proof(&package, &decoded, true), "reencoded {language}: {changed}");
        }
    }
}

// 이것을 실패시키는 것: local name만 비교, 원문 참조 누락·Core index 불일치를 승인.
#[test]
fn opening_references_require_bound_names_and_explicit_indices() {
    let (package, decoded) = wire_fixture();
    let xml = package.read_text_entry("Contents/section0.xml").unwrap();
    let section = SectionEvidence::read(&xml).unwrap();
    let header = Header::read(&package, &decoded.style_store, || {}).unwrap();
    let paragraph = &decoded.document.sections()[0].paragraphs[0];
    let mut missing_style = paragraph.clone();
    missing_style.style_ref = None;
    let source = references(
        "<hp:p paraPrIDRef='0' styleIDRef='0'>",
        ["<hp:run charPrIDRef='0'>"],
        &section.namespaces,
    )
    .unwrap();
    assert!(!header.proves(&section, &source, &missing_style, &decoded.style_store, false));
    for uri in [namespace("p"), "urn:foreign"] {
        let mut ns = section.namespaces.clone();
        ns.insert("alias".into(), uri.into());
        let source = references(
            "<alias:p paraPrIDRef='0' styleIDRef='0'>",
            ["<alias:run charPrIDRef='0'>"],
            &ns,
        );
        assert_eq!(source.is_some(), uri == namespace("p"));
    }
    for (p, r) in [
        ("<hp:p styleIDRef='0'>", "<hp:run charPrIDRef='0'>"),
        ("<hp:p paraPrIDRef='0'>", "<hp:run charPrIDRef='0'>"),
        ("<hp:p paraPrIDRef='0' styleIDRef='0'>", "<hp:run>"),
        ("<hp:p paraPrIDRef='1' styleIDRef='0'>", "<hp:run charPrIDRef='0'>"),
        ("<hp:p paraPrIDRef='0' styleIDRef='1'>", "<hp:run charPrIDRef='0'>"),
        ("<hp:p paraPrIDRef='0' styleIDRef='0'>", "<hp:run charPrIDRef='1'>"),
    ] {
        assert!(
            !references(p, [r], &section.namespaces).is_some_and(|source| header.proves(
                &section,
                &source,
                paragraph,
                &decoded.style_store,
                false
            )),
            "{p} {r}"
        );
    }
}

// 이것을 실패시키는 것: header 요소 local name만 비교, 정의 목록에서 외래 URI 요소를 건너뛰어 위치 이동.
#[test]
fn header_aliases_preserve_positions_but_foreign_definitions_close_the_proof() {
    let (mut package, decoded) = wire_fixture();
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    package.replace_text_entry(
        HEADER_PART,
        xml.replace("hh:", "alias:").replace("xmlns:hh=", "xmlns:alias="),
    );
    assert!(proof(&package, &decoded, true));
    for element in ["charPr", "paraPr", "style", "font", "typeInfo", "fontface"] {
        let foreign = xml
            .replace("<hh:head ", "<hh:head xmlns:alien=\"urn:foreign\" ")
            .replace(&format!("<hh:{element} "), &format!("<alien:{element} "))
            .replace(&format!("</hh:{element}>"), &format!("</alien:{element}>"));
        package.replace_text_entry(HEADER_PART, foreign);
        // Style definitions do not supply the directly resolved paragraph/run values.
        // 이것을 실패시키는 것: 스타일 정의 namespace 대조 복원.
        assert_eq!(proof(&package, &decoded, true), element == "style", "{element}");
    }
}

// 이것을 실패시키는 것: 불명/중복 문단 선택 분기를 Core 기본값으로 덮어 승인.
#[test]
fn ambiguous_paragraph_branches_cannot_supply_default_evidence() {
    let (mut package, decoded) = wire_fixture();
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    for extra in [
        "<hh:border/><hh:border/>",
        "<hh:margin/><hh:margin/>",
        "<hh:lineSpacing/><hh:lineSpacing/>",
        "<hp:switch/>",
    ] {
        package.replace_text_entry(
            HEADER_PART,
            xml.replace("</hh:paraPr>", &format!("{extra}</hh:paraPr>")),
        );
        assert!(!proof(&package, &decoded, false), "{extra}");
    }
}

// 이것을 실패시키는 것: height 양수 조건·원문과 저장소의 중립값 대조·문단 인코더 수치 preflight 생략.
#[test]
fn invalid_measurements_close_the_proof_without_encoder_panics() {
    for height in [0, -1] {
        let shape = HwpxCharShape {
            height: hwpforge_foundation::HwpUnit::new(height).unwrap(),
            ..Default::default()
        };
        let mut tag = character();
        tag.attrs.insert("height".into(), height.to_string());
        assert!(character_allows(&tag, &shape).is_none());
    }
    let (package, mut decoded) = wire_fixture();
    for font_line_height in [true, false] {
        let (_, mut changed) = wire_fixture();
        let p = changed
            .style_store
            .para_shape_mut(hwpforge_foundation::ParaShapeIndex::new(0))
            .unwrap();
        if font_line_height {
            p.font_line_height = 1;
        } else {
            p.condense = 1;
        }
        assert!(!proof(&package, &changed, false));
    }
    let shape =
        decoded.style_store.para_shape_mut(hwpforge_foundation::ParaShapeIndex::new(0)).unwrap();
    shape.line_spacing = i32::MAX;
    shape.line_spacing_type = hwpforge_foundation::LineSpacingType::Fixed;
    assert!(Header::read(&package, &decoded.style_store, || {}).is_none());
}

// Style definitions do not supply the directly resolved paragraph/run values.
// 이것을 실패시키는 것: 스타일 paraPr/charPr 대조 또는 PARA 타입 요구 복원.
#[test]
fn style_definitions_do_not_override_direct_formatting_evidence() {
    let (mut package, decoded) = wire_fixture();
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    for (from, to) in [
        ("paraPrIDRef=\"0\"", "paraPrIDRef=\"1\""),
        ("charPrIDRef=\"0\"", "charPrIDRef=\"1\""),
        ("type=\"PARA\"", "type=\"CHAR\""),
    ] {
        assert!(xml.contains(from));
        package.replace_text_entry(HEADER_PART, xml.replace(from, to));
        assert!(proof(&package, &decoded, false), "{from}");
        assert!(proof(&package, &decoded, true), "{from}");
    }
}

// 이것을 실패시키는 것: 스타일의 간접 paraPr/charPr와 직접 참조 일치 요구 복원.
#[test]
fn direct_paragraph_and_character_references_can_differ_from_the_style() {
    let (mut package, _) = wire_fixture();
    let mut header = package.read_text_entry(HEADER_PART).unwrap();
    for name in ["paraPr", "charPr"] {
        let start = header.find(&format!("<hh:{name} ")).unwrap();
        let close = format!("</hh:{name}>");
        let end = start + header[start..].find(&close).unwrap() + close.len();
        let extra = header[start..end].replacen("id=\"0\"", "id=\"1\"", 1);
        header.insert_str(end, &extra);
    }
    package.replace_text_entry(HEADER_PART, header);
    let xml = package
        .read_text_entry("Contents/section0.xml")
        .unwrap()
        .replace("paraPrIDRef=\"0\"", "paraPrIDRef=\"1\"")
        .replace("charPrIDRef=\"0\"", "charPrIDRef=\"1\"");
    package.replace_text_entry("Contents/section0.xml", xml.clone());
    let decoded = crate::HwpxDecoder::decode(&package.write().unwrap()).unwrap();
    let paragraph = &decoded.document.sections()[0].paragraphs[0];
    assert_eq!(paragraph.para_shape_id.get(), 1);
    assert_eq!(paragraph.runs[0].char_shape_id.get(), 1);
    let style = decoded.style_store.style(0).unwrap();
    assert_eq!((style.para_pr_id_ref, style.char_pr_id_ref), (0, 0));
    let section = SectionEvidence::read(&xml).unwrap();
    let source = references(
        "<hp:p paraPrIDRef='1' styleIDRef='0'>",
        ["<hp:run charPrIDRef='1'>"],
        &section.namespaces,
    )
    .unwrap();
    let header = Header::read(&package, &decoded.style_store, || {}).unwrap();
    for reencoded in [false, true] {
        assert!(header.proves(&section, &source, paragraph, &decoded.style_store, reencoded));
    }
}

// 이것을 실패시키는 것: Header::read에서 글꼴 대조를 미리 계산하거나 raw 소비자가 초기화.
#[test]
fn font_evidence_is_initialized_only_by_a_reencoded_consumer() {
    let (package, decoded) = wire_fixture();
    let header = Header::read(&package, &decoded.style_store, || {}).unwrap();
    assert!(header.fonts.get().is_none());
    let section =
        SectionEvidence::read(&package.read_text_entry("Contents/section0.xml").unwrap()).unwrap();
    let source = references(
        "<hp:p paraPrIDRef='0' styleIDRef='0'>",
        ["<hp:run charPrIDRef='0'>"],
        &section.namespaces,
    )
    .unwrap();
    let paragraph = &decoded.document.sections()[0].paragraphs[0];
    assert!(header.proves(&section, &source, paragraph, &decoded.style_store, false));
    assert!(header.fonts.get().is_none());
    assert!(header.proves(&section, &source, paragraph, &decoded.style_store, true));
    let cached = header.fonts.get().unwrap().as_ptr();
    assert!(header.proves(&section, &source, paragraph, &decoded.style_store, true));
    assert_eq!(header.fonts.get().unwrap().as_ptr(), cached);
}

// 이것을 실패시키는 것: 양쪽 typeInfo 부재 거부, 원문/저장소 한쪽 부재를 승인.
#[test]
fn font_type_info_absence_must_agree_in_both_directions() {
    let (package, present) = wire_fixture();
    let xml = package.read_text_entry(HEADER_PART).unwrap();
    let mut missing_xml = xml.clone();
    while let Some(start) = missing_xml.find("<hh:typeInfo") {
        let end = start + missing_xml[start..].find("/>").unwrap() + 2;
        missing_xml.replace_range(start..end, "");
    }
    let mut missing_package = RawPackage::read(&package.write().unwrap()).unwrap();
    missing_package.replace_text_entry(HEADER_PART, missing_xml);
    let missing = crate::HwpxDecoder::decode(&missing_package.write().unwrap()).unwrap();
    assert!(missing.style_store.iter_fonts().all(|f| f.type_info.is_none()));
    for (raw, stored, expected) in [
        (&package, &present, true),
        (&missing_package, &missing, true),
        (&missing_package, &present, false),
        (&package, &missing, false),
    ] {
        assert!(proof(raw, stored, false));
        assert_eq!(proof(raw, stored, true), expected);
    }
}

// 이것을 실패시키는 것: 하위 바인딩을 형제에 누출, 범위가 끝난 뒤 바깥 바인딩 미복원, name에 URI 복원.
#[test]
fn wide_deep_namespace_trees_keep_bindings_scoped() {
    // Bindings live on the parser's stack only while their element is open.
    let mut xml = format!("<hs:sec xmlns:hs='{}' xmlns:hp='{}'", namespace("sec"), namespace("p"));
    for i in 0..128 {
        xml.push_str(&format!(" xmlns:alias{i}='urn:{i}'"));
    }
    xml.push('>');
    for i in 0..96 {
        xml.push_str(&format!("<hp:p xmlns:alias{i}='urn:inner{i}'>"));
    }
    xml.push_str(
        "<hp:p xmlns:hp='urn:foreign'/><hp:p/><hp:p xmlns:hp='urn:foreign'><hp:p/></hp:p><hp:p/>",
    );
    for _ in 0..4096 {
        xml.push_str("<hp:p/>");
    }
    for _ in 0..96 {
        xml.push_str("</hp:p>");
    }
    xml.push_str("<hp:p/></hs:sec>");
    let root = Tag::read(&xml).unwrap();
    assert_eq!(root.name, "sec");
    assert_eq!(root.children[1].name, "p");
    let mut node = &root;
    for _ in 0..96 {
        node = &node.children[0];
        assert_eq!(node.name, "p");
        assert!(node.attrs.is_empty());
    }
    assert_eq!(node.children.len(), 4100);
    assert_eq!(node.children[0].name, "{foreign}p");
    assert_eq!(node.children[1].name, "p");
    assert_eq!(node.children[2].name, "{foreign}p");
    assert_eq!(node.children[2].children[0].name, "{foreign}p");
    assert!(node.children[3..].iter().all(|t| t.name == "p" && t.attrs.is_empty()));
}

// 이것을 실패시키는 것: 외래 grid를 명시 거부하지 않고 정상 grid 하나로 인정.
#[test]
fn foreign_grid_is_explicitly_rejected_by_both_collectors() {
    let (package, _) = wire_fixture();
    let xml =
        package.read_text_entry("Contents/section0.xml").unwrap().replace("<hp:grid ", "<hh:grid ");
    let section = SectionEvidence::read(&xml).unwrap();
    assert_eq!(section.grids, 1);
    assert!(section.foreign_grid);
    assert!(!source_allows(&section, true));
    let (_, source) = hwpforge_core::source_xml::XmlSource::read(&xml).unwrap();
    let mut flow = crate::cache_flow::ParagraphFlow::from_source(&source);
    flow.prove_text_tail(&source, &xml, &Default::default(), |_| None);
    let section = flow.section_evidence(&xml).unwrap();
    assert_eq!(section.grids, 1);
    assert!(section.foreign_grid);
    assert!(!source_allows(&section, true));
}

// 이것을 실패시키는 것: Tag에 URI 문자열 필드 복원 또는 외래 name에 URI 복사.
#[test]
fn long_namespace_is_not_retained_by_each_element() {
    // One name, one attribute map, one child vector and two flags; no URI field.
    assert_eq!(
        std::mem::size_of::<Tag>(),
        std::mem::size_of::<String>()
            + std::mem::size_of::<HashMap<String, String>>()
            + std::mem::size_of::<Vec<Tag>>()
            + std::mem::align_of::<Tag>()
    );
    let small = Tag::read("<x:extension xmlns:x='urn:foreign'/>").unwrap();
    assert_eq!(small.name, "{foreign}extension");
    assert!(!small.bound_namespace);
    let (mut package, decoded) = wire_fixture();
    let uri = "x".repeat(1024 * 1024);
    let extensions = "<x:extension/>".repeat(4096);
    let xml = package
        .read_text_entry(HEADER_PART)
        .unwrap()
        .replace("<hh:head ", &format!("<hh:head xmlns:x='{uri}' "))
        .replace("</hh:head>", &format!("{extensions}</hh:head>"));
    let root = Tag::read(&xml).unwrap();
    let extensions: Vec<_> =
        root.children.iter().filter(|t| t.name.ends_with("extension")).collect();
    assert_eq!(extensions.len(), 4096);
    for tag in extensions {
        assert_eq!(tag.name, "{foreign}extension");
        assert!(!tag.bound_namespace);
        assert!(!tag.declares_namespace);
        assert!(tag.attrs.is_empty() && tag.children.is_empty());
    }
    package.replace_text_entry(HEADER_PART, xml.clone());
    assert!(proof(&package, &decoded, true));
    // A relevant foreign definition still closes the proof with the same URI.
    package.replace_text_entry(
        HEADER_PART,
        xml.replace("<hh:charPr ", "<x:charPr ").replace("</hh:charPr>", "</x:charPr>"),
    );
    assert!(!proof(&package, &decoded, true));
}

// 이것을 실패시키는 것: p/run/secPr 중 외래 요소를 건너뛰고 정상 속성만 유일하다고 인정.
#[test]
fn mixed_foreign_section_carriers_are_rejected_by_both_collectors() {
    let (package, _) = wire_fixture();
    let original = package.read_text_entry("Contents/section0.xml").unwrap();
    for carrier in ["p", "run", "secPr"] {
        for foreign in [false, true] {
            let extra = "<hp:p><hp:run><hp:secPr textDirection='HORIZONTAL'><hp:grid charGrid='0' lineGrid='9' wonggojiFormat='0'/></hp:secPr></hp:run></hp:p>";
            let extra = extra.replace(&format!("hp:{carrier}"), &format!("x:{carrier}"));
            let uri = if foreign { "urn:foreign" } else { namespace("p") };
            let xml = original
                .replace("<hs:sec ", &format!("<hs:sec xmlns:x='{uri}' "))
                .replace("</hs:sec>", &format!("{extra}</hs:sec>"));
            let cell = SectionEvidence::read(&xml);
            let (_, source) = hwpforge_core::source_xml::XmlSource::read(&xml).unwrap();
            let mut flow = crate::cache_flow::ParagraphFlow::from_source(&source);
            flow.prove_text_tail(&source, &xml, &Default::default(), |_| None);
            let patch = flow.section_evidence(&xml);
            for evidence in [cell, patch] {
                if foreign {
                    assert!(evidence.is_none(), "foreign {carrier}");
                } else {
                    let evidence = evidence.unwrap();
                    assert_eq!((evidence.properties, evidence.grids), (2, 2));
                    assert!(!source_allows(&evidence, true));
                }
            }
        }
    }
}

// 이것을 실패시키는 것: 글자 정의마다 원문 글꼴 목록 재구성 또는 저장소 목록 재순회.
#[test]
fn font_evidence_work_is_linear_in_fonts_plus_characters() {
    use hwpforge_core::{Document, ImageStore, PageSettings, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, FontIndex, ParaShapeIndex};
    for (font_count, char_count) in [(8, 16), (128, 16), (128, 128)] {
        let mut styles = HwpxStyleStore::new();
        for language in LANGUAGES {
            for index in 0..font_count {
                styles.push_font(crate::style_store::HwpxFont::new(
                    index as u32,
                    format!("{language}{index}"),
                    language,
                ));
            }
        }
        let last = FontIndex::new(font_count - 1);
        let shape = HwpxCharShape {
            font_ref: crate::style_store::HwpxFontRef {
                hangul: last,
                latin: last,
                hanja: last,
                japanese: last,
                other: last,
                symbol: last,
                user: last,
            },
            ..Default::default()
        };
        for _ in 0..char_count {
            styles.push_char_shape(shape.clone());
        }
        styles.push_para_shape(crate::style_store::HwpxParaShape::default());
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("가", CharShapeIndex::new(char_count - 1))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let bytes =
            crate::HwpxEncoder::encode(&document.validate().unwrap(), &styles, &ImageStore::new())
                .unwrap();
        let package = RawPackage::read(&bytes).unwrap();
        let decoded = crate::HwpxDecoder::decode(&bytes).unwrap();
        let header = Header::read(&package, &decoded.style_store, || {}).unwrap();
        crate::work::WORK.set(0);
        let fonts = header.fonts(&decoded.style_store);
        assert_eq!(fonts.len(), char_count);
        assert!(fonts.iter().all(|&allowed| allowed));
        // Source traversal + store traversal + metadata, then seven lookups per character.
        assert_eq!(crate::work::WORK.get(), 7 * (3 * font_count + char_count));
        assert_eq!(header.fonts(&decoded.style_store), fonts);
        assert_eq!(crate::work::WORK.get(), 7 * (3 * font_count + char_count));
    }
}

// 이것을 실패시키는 것: 지연 글꼴 검사용 트리를 줄일 때 외래 fontfaces를 버려 유일성 인정.
#[test]
fn mixed_foreign_fontfaces_remain_visible_to_the_deferred_check() {
    let (mut package, decoded) = wire_fixture();
    let xml = package
        .read_text_entry(HEADER_PART)
        .unwrap()
        .replace("<hh:head ", "<hh:head xmlns:x='urn:foreign' ")
        .replace("</hh:refList>", "<x:fontfaces/></hh:refList>");
    package.replace_text_entry(HEADER_PART, xml);
    assert!(proof(&package, &decoded, false));
    assert!(!proof(&package, &decoded, true));
}

// 이것을 실패시키는 것: 상속 바인딩을 opening_in의 선언 목록에 복제하거나,
// opening_with_bindings에서 공유 문자열을 깊은 복사로 되돌리기.
#[test]
fn long_namespace_many_runs_borrow_or_share_inherited_bindings() {
    const RUNS: usize = 4096;
    let paragraph = "<hp:p paraPrIDRef='0' styleIDRef='0'>";
    let run = "<hp:run charPrIDRef='0'/>";
    let xml = format!(
        "<hs:sec xmlns:hs='{}' xmlns:hp='{}' xmlns:unused='{}'>{paragraph}{}</hp:p></hs:sec>",
        namespace("sec"),
        namespace("p"),
        "x".repeat(1024 * 1024),
        run.repeat(RUNS),
    );
    let section = SectionEvidence::read(&xml).unwrap();
    let (_, bindings) = Tag::opening_with_bindings(paragraph, &section.namespaces).unwrap();
    // Both retained adapters must share all inherited keys and values, including unused URIs.
    for retained in [&bindings, &section.clone().namespaces] {
        for (prefix, uri) in &section.namespaces {
            let (key, value) = retained.get_key_value(prefix).unwrap();
            assert!(Rc::ptr_eq(prefix, key));
            assert!(Rc::ptr_eq(uri, value));
        }
    }
    for _ in 0..RUNS {
        let mut declarations = Vec::new();
        let tag = Tag::opening_in(run, &bindings, &mut declarations).unwrap();
        assert_eq!(tag.name, "run");
        // The shared opening parser (also used by references) only stores local declarations.
        assert!(declarations.is_empty());
    }
    START_TAG_READS.with(|n| n.set(0));
    let refs = references(paragraph, std::iter::repeat_n(run, RUNS), &section.namespaces).unwrap();
    assert_eq!(refs.characters, vec![0; RUNS]);
    START_TAG_READS.with(|n| assert_eq!(n.get(), RUNS + 1));
    // Exercise the patch adapter with decoder-owned ranges as well as the cell collector above.
    let (_, source) = hwpforge_core::source_xml::XmlSource::read(&xml).unwrap();
    let mut flow = crate::cache_flow::ParagraphFlow::from_source(&source);
    flow.prove_text_tail(&source, &xml, &Default::default(), |_| None);
    let patch_section = flow.section_evidence(&xml).unwrap();
    START_TAG_READS.with(|n| n.set(0));
    assert_eq!(flow.references(0, &xml, &patch_section.namespaces), Some(refs));
    START_TAG_READS.with(|n| assert_eq!(n.get(), RUNS + 1));
}

// 이것을 실패시키는 것: 로컬 재선언보다 상속 바인딩을 우선하거나 상속 fallback 누락.
#[test]
fn borrowed_namespace_scope_restores_inherited_bindings() {
    let namespaces = NamespaceBindings::from([
        (Rc::from("hp"), Rc::from(namespace("p"))),
        (Rc::from(""), Rc::from(namespace("p"))),
    ]);
    let xml = "<hp:p><hp:run xmlns:hp='urn:foreign'/><hp:run/><hp:run xmlns:hp='urn:foreign'><hp:run/></hp:run><hp:run/><run xmlns=''/><run/></hp:p>";
    let root = Tag::read_in(xml, &namespaces).unwrap();
    assert!(root.bound_namespace);
    assert_eq!(root.children[0].name, "{foreign}run");
    assert!(root.children[1].bound_namespace);
    assert_eq!(root.children[2].name, "{foreign}run");
    assert_eq!(root.children[2].children[0].name, "{foreign}run");
    assert!(root.children[3].bound_namespace);
    assert!(!root.children[4].bound_namespace);
    assert!(root.children[5].bound_namespace);
    let (_, rebound) =
        Tag::opening_with_bindings("<hp:p xmlns:hp='urn:foreign'>", &namespaces).unwrap();
    assert_eq!(Tag::opening("<hp:run/>", &rebound).unwrap().name, "{foreign}run");
    assert_eq!(Tag::opening("<hp:run/>", &namespaces).unwrap().name, "run");
}
