use super::*;

fn script_seed() -> Vec<u8> {
    let mut doc = Document::new();
    for label in ["첫", "둘"] {
        doc.add_section(Section::with_paragraphs(
            ["시작", "삭제", "생존"].map(|text| paragraph(&format!("{label} {text}"))).to_vec(),
            PageSettings::a4(),
        ));
    }
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let mut p = RawPackage::read(&base).unwrap();
    let content = p
        .read_text_entry("Contents/content.hpf")
        .unwrap()
        .replace("\"section0\"", "\"body-a\"")
        .replace("\"section1\"", "\"body-b\"");
    p.replace_text_entry("Contents/content.hpf", content);
    p.write().unwrap()
}

fn scripts(base: &[u8], refs: &str) -> Vec<u8> {
    let base = extra(
        &extra(base, "Scripts/headerScripts.js", b"/\0/\0h\0"),
        "Scripts/sourceScripts.js",
        b"/\0/\0s\0",
    );
    let base = replace(&base, "Contents/content.hpf", "</opf:manifest>",
        "<opf:item id='aux-h' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/><opf:item id='aux-s' href='Scripts/sourceScripts.js' media-type='application/x-javascript ;charset=utf-16'/></opf:manifest>");
    replace(&base, "Contents/content.hpf", "<opf:spine>", &format!("<opf:spine>{refs}"))
}

fn script_input() -> Vec<u8> {
    scripts(&script_seed(), "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>")
}

fn no_script_evidence(base: &[u8], target: ParagraphLocator) {
    let expected = original_error(base);
    assert_eq!(HwpxStructuralEditor::delete_paragraphs(base, &[target]).unwrap_err(), expected);
    let d = HwpxDecoder::decode(base).unwrap();
    assert!(d
        .source_basis
        .as_ref()
        .unwrap()
        .losses
        .iter()
        .all(|loss| loss.cause != crate::decoder::preserved_loss::LossCause::ScriptAuxiliaryOrder));
}

// 이것을 실패시키는 것: 알려진 두 경로 중 하나 누락 또는 script no 거부.
#[test]
fn script_known_paths_and_linear() {
    for first in ["", " linear='yes'", " linear='no'"] {
        for second in ["", " linear='yes'", " linear='no'"] {
            for refs in [
                format!("<opf:itemref idref='aux-h'{first}/>"),
                format!("<opf:itemref idref='aux-s'{second}/>"),
                format!("<opf:itemref idref='aux-h'{first}/><opf:itemref idref='aux-s'{second}/>"),
            ] {
                for seed in [script_seed(), header_order(&script_seed())] {
                    let input = scripts(&seed, &refs);
                    succeeds(&input);
                    let d = HwpxDecoder::decode(&input).unwrap();
                    assert_eq!(d.source_basis.as_ref().unwrap().losses.len(), 1);
                    assert_eq!(
                        d.source_basis.as_ref().unwrap().losses[0].cause,
                        crate::decoder::preserved_loss::LossCause::ScriptAuxiliaryOrder
                    );
                    assert_eq!(d.warnings.len(), 1);
                    assert!(d.warnings[0].is_content_loss());
                }
            }
        }
    }
}

// 이것을 실패시키는 것: script 순번을 section 주소에 포함 또는 section 배열 반전.
#[test]
fn script_positions_keep_section_addresses() {
    for order in [
        ["aux-h", "aux-s", "header", "body-a", "body-b"],
        ["header", "body-a", "aux-s", "aux-h", "body-b"],
        ["header", "body-a", "body-b", "aux-h", "aux-s"],
    ] {
        let base = scripts(&script_seed(), "");
        let mut pkg = RawPackage::read(&base).unwrap();
        let content = pkg.read_text_entry("Contents/content.hpf").unwrap();
        let start = content.find("<opf:spine>").unwrap() + 11;
        let end = content.find("</opf:spine>").unwrap();
        let refs = order.map(|id| format!("<opf:itemref idref='{id}'/>")).concat();
        pkg.replace_text_entry(
            "Contents/content.hpf",
            format!("{}{refs}{}", &content[..start], &content[end..]),
        );
        let input = pkg.write().unwrap();
        let d = HwpxDecoder::decode(&input).unwrap();
        assert_eq!(pkg.section_path(0).unwrap(), "Contents/section0.xml");
        for section in 0..2 {
            let target = ParagraphLocator { section, index: 1 };
            let out =
                HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&input, &[target])
                    .unwrap();
            assert_eq!(out.decode_warnings, d.warnings);
            let after = HwpxDecoder::decode(&out.value).unwrap();
            let mut expected = d.document.clone();
            expected.sections_mut()[section].paragraphs.remove(1);
            assert_eq!(after.document, expected);
            let p = RawPackage::read(&out.value).unwrap();
            assert_eq!(p.section_path(0).unwrap(), "Contents/section0.xml");
            assert_eq!(p.section_path(1).unwrap(), "Contents/section1.xml");
            let part = format!("Contents/section{section}.xml");
            let source = pkg.read_text_entry(&part).unwrap();
            let span = d.source_maps[section].paragraphs[1].outer.clone();
            assert_eq!(
                p.read_text_entry(&part).unwrap(),
                format!("{}{}", &source[..span.start], &source[span.end..])
            );
            for path in [
                "Contents/content.hpf",
                "Contents/header.xml",
                "Scripts/headerScripts.js",
                "Scripts/sourceScripts.js",
            ] {
                assert_eq!(pkg.read_entry(path).unwrap(), p.read_entry(path).unwrap());
            }
        }
    }
}

// 이것을 실패시키는 것: 허용 비트만으로 script 원인 선택.
#[test]
fn header_only_cause_is_stable() {
    let seed = scripts(&script_seed(), "");
    let duplicate = replace(&seed, "Contents/content.hpf", "</opf:manifest>", "<opf:item id='unused' href='Scripts/headerScripts.js' media-type='application/xml'/><opf:item id='unused' href='Scripts/sourceScripts.js' media-type='application/xml'/></opf:manifest>");
    for base in [script_seed(), seed, duplicate] {
        let input = header_order(&base);
        succeeds(&input);
        let d = HwpxDecoder::decode(&input).unwrap();
        assert_eq!(d.source_basis.as_ref().unwrap().losses.len(), 1);
        assert_eq!(
            d.source_basis.as_ref().unwrap().losses[0].cause,
            crate::decoder::preserved_loss::LossCause::HeaderOnlyNonlinearOrder
        );
    }
}

// 이것을 실패시키는 것: script 근거에서 manifest의 같은 href 별칭 검사 제거.
#[test]
fn script_unreferenced_manifest_alias_is_refused() {
    let input = scripts(&script_seed(), "<opf:itemref idref='aux-h'/>");
    let input = replace(&input, "Contents/content.hpf", "</opf:manifest>",
        "<opf:item id='alias' href='Scripts/headerScripts.js' media-type='application/xml'/></opf:manifest>");
    no_script_evidence(&input, TARGET);
}

// 이것을 실패시키는 것: script 허용 모드에서 section/masterpage no까지 승인.
#[test]
fn script_section_and_masterpage_no_refused() {
    for (body, section) in [("body-a", 0), ("body-b", 0), ("body-a", 1)] {
        let input = replace(
            &script_input(),
            "Contents/content.hpf",
            &format!("idref=\"{body}\" linear=\"yes\""),
            &format!("idref=\"{body}\" linear=\"no\""),
        );
        no_script_evidence(&input, ParagraphLocator { section, index: 1 });
    }
    use hwpforge_core::section::MasterPage;
    use hwpforge_foundation::ApplyPageType;
    let mut section = Section::with_paragraphs(
        vec![paragraph("시작"), paragraph("삭제"), paragraph("생존")],
        PageSettings::a4(),
    );
    section.master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![paragraph("바탕")])]);
    let mut doc = Document::new();
    doc.add_section(section);
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = scripts(&base, "<opf:itemref idref='aux-h'/>");
    let base = replace(
        &base,
        "Contents/content.hpf",
        "idref=\"masterpage0\" linear=\"yes\"",
        "idref='masterpage0' linear='no'",
    );
    no_script_evidence(&base, TARGET);
}

// 이것을 실패시키는 것: yes 이외 값을 모두 no로 정규화.
#[test]
fn script_unknown_linear_refused() {
    for (id, needle) in [("aux-h", "idref='aux-h'"), ("header", "idref=\"header\" linear=\"yes\"")]
    {
        for value in ["", "false", "NO"] {
            let input = scripts(&script_seed(), "<opf:itemref idref='aux-h'/>");
            let input = replace(
                &input,
                "Contents/content.hpf",
                needle,
                &format!("idref='{id}' linear='{value}'"),
            );
            no_script_evidence(&input, TARGET);
        }
    }
}

// 이것을 실패시키는 것: 경로 확장자 또는 정규화 MIME 비교로 허용 확대.
#[test]
fn script_exact_path_and_media_only() {
    for path in [
        "Scripts/other.js",
        "scripts/headerScripts.js",
        "./Scripts/headerScripts.js",
        "Scripts/sub/headerScripts.js",
    ] {
        let base = scripts(&script_seed(), "<opf:itemref idref='aux-h'/>");
        let mut pkg = RawPackage::read(&base).unwrap();
        pkg.remove_entries_exact(&["Scripts/headerScripts.js".into()]);
        let base = extra(&pkg.write().unwrap(), path, b"/\0/\0h\0");
        let base = replace(&base, "Contents/content.hpf", "Scripts/headerScripts.js", path);
        no_script_evidence(&base, TARGET);
    }
    for media in [
        "application/x-javascript",
        "application/xml",
        "text/javascript",
        "application/x-javascript;charset=utf-16",
        "application/x-javascript ;charset=UTF-16",
        "Application/x-javascript ;charset=utf-16",
        "application/x-javascript ;charset=utf-16 ",
    ] {
        no_script_evidence(
            &replace(
                &scripts(&script_seed(), "<opf:itemref idref='aux-h'/>"),
                "Contents/content.hpf",
                "application/x-javascript ;charset=utf-16",
                media,
            ),
            TARGET,
        );
    }
}

// 이것을 실패시키는 것: script를 존재/seen 검사 전에 건너뜀.
#[test]
fn script_targets_exist_and_are_unique() {
    let good = script_input();
    let mut pkg = RawPackage::read(&good).unwrap();
    pkg.remove_entries_exact(&["Scripts/headerScripts.js".into()]);
    no_script_evidence(&pkg.write().unwrap(), TARGET);
    for (from, to) in [
        ("id='aux-h'", "id='missing'"),
        ("</opf:spine>", "<opf:itemref idref='aux-h'/></opf:spine>"),
        ("Scripts/sourceScripts.js", "Scripts/headerScripts.js"),
    ] {
        no_script_evidence(&replace(&good, "Contents/content.hpf", from, to), TARGET);
    }
}

// 이것을 실패시키는 것: manifest 마지막 값 덮어쓰기 또는 미참조 중복 ID 승인.
#[test]
fn script_manifest_ids_are_unique() {
    for id in ["aux-h", "body-a", "unreferenced"] {
        for href in ["Scripts/headerScripts.js", "Contents/section0.xml"] {
            let item = format!("<opf:item id='{id}' href='{href}' media-type='application/xml'/>");
            let input = replace(
                &script_input(),
                "Contents/content.hpf",
                "</opf:manifest>",
                &format!("{item}{item}</opf:manifest>"),
            );
            no_script_evidence(&input, TARGET);
        }
    }
}

// 이것을 실패시키는 것: section 포함 개수만 검사 또는 재시도 section 정렬.
#[test]
fn script_sections_cover_fallback_exactly() {
    for to in ["", "<opf:itemref idref='body-a'/>", "<opf:itemref idref='alias'/>"] {
        let base = replace(&script_input(), "Contents/content.hpf", "</opf:manifest>", "<opf:item id='alias' href='Contents/section0.xml' media-type='application/xml'/></opf:manifest>");
        let input = replace(
            &base,
            "Contents/content.hpf",
            "<opf:itemref idref=\"body-b\" linear=\"yes\"/>",
            to,
        );
        no_script_evidence(&input, TARGET);
    }
    let base = script_input();
    let input = replace(&base, "Contents/content.hpf", "<opf:itemref idref=\"body-a\" linear=\"yes\"/><opf:itemref idref=\"body-b\" linear=\"yes\"/>", "<opf:itemref idref='body-b'/><opf:itemref idref='body-a'/>");
    no_script_evidence(&input, TARGET);
    assert_eq!(RawPackage::read(&input).unwrap().section_path(0).unwrap(), "Contents/section0.xml");
}

// 이것을 실패시키는 것: 선택 속성 검사와 중복 XML 속성 검사 생략.
#[test]
fn script_selection_attributes_refused() {
    for (part, tag) in [
        ("Contents/content.hpf", "<opf:package "),
        ("Contents/content.hpf", "<opf:manifest>"),
        ("Contents/content.hpf", "<opf:item "),
        ("Contents/content.hpf", "<opf:spine>"),
        ("Contents/content.hpf", "<opf:itemref "),
        ("META-INF/container.xml", "<ocf:container "),
        ("META-INF/container.xml", "<ocf:rootfiles>"),
        ("META-INF/container.xml", "<ocf:rootfile "),
    ] {
        for attr in [
            "xml:base='x'",
            "required-namespace='x'",
            "required-modules='x'",
            "fallback='x'",
            "fallback-style='x'",
            "requiredFeatures='x'",
            "requiredExtensions='x'",
            "systemLanguage='x'",
            "xmlns:z='urn:z' z:choice='x'",
        ] {
            let replacement =
                format!("{} {attr}{}", tag.trim_end_matches([' ', '>']), &tag[tag.len() - 1..]);
            no_script_evidence(&replace(&script_input(), part, tag, &replacement), TARGET);
        }
    }
    no_script_evidence(
        &replace(
            &script_input(),
            "Contents/content.hpf",
            "idref='aux-h'",
            "idref='aux-h' idref='aux-h'",
        ),
        TARGET,
    );
}

// 이것을 실패시키는 것: container/content 제외 가능 조건 생략.
#[test]
fn script_excludable_gate_remains() {
    for (part, tag) in [
        ("Contents/content.hpf", "<opf:package "),
        ("Contents/content.hpf", "<opf:manifest>"),
        ("Contents/content.hpf", "<opf:item "),
        ("Contents/content.hpf", "<opf:spine>"),
        ("Contents/content.hpf", "<opf:itemref "),
        ("META-INF/container.xml", "<ocf:container "),
        ("META-INF/container.xml", "<ocf:rootfiles>"),
        ("META-INF/container.xml", "<ocf:rootfile "),
    ] {
        let replacement =
            format!("{} future='1'{}", tag.trim_end_matches([' ', '>']), &tag[tag.len() - 1..]);
        no_script_evidence(&replace(&script_input(), part, tag, &replacement), TARGET);
    }
}

// 이것을 실패시키는 것: metadata 실패를 경고 존재만으로 추측.
#[test]
fn script_metadata_failure_is_not_hidden() {
    let good = script_input();
    for child in [
        "<opf:title><![CDATA[x]]></opf:title>",
        "<title>x</title>",
        "<z:title xmlns:z='urn:z'>x</z:title>",
    ] {
        let bad = replace(&good, "Contents/content.hpf", "<opf:title/>", child);
        no_script_evidence(&bad, TARGET);
    }
    let bad = replace(
        &good,
        "Contents/content.hpf",
        "<opf:title/>",
        "<opf:title><![CDATA[x]]></opf:title>",
    );
    assert_eq!(
        HwpxDecoder::decode(&bad).unwrap().warnings,
        HwpxDecoder::decode(&good).unwrap().warnings
    );
}

// 이것을 실패시키는 것: package 근거 하나로 다른 손실까지 면제.
#[test]
fn script_metadata_drop_and_other_losses_refused() {
    let base = script_input();
    for input in [
        replace(
            &base,
            "Contents/content.hpf",
            "<opf:title/>",
            &format!("<opf:title>{}</opf:title>", "x".repeat(65_537)),
        ),
        replace(&base, "Contents/section0.xml", "<hp:t>첫 생존", "<hp:unknown/><hp:t>첫 생존"),
    ] {
        let d = HwpxDecoder::decode(&input).unwrap();
        assert_eq!(d.source_basis.as_ref().unwrap().losses.len(), 1);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
            original_error(&input)
        );
    }
}

// 이것을 실패시키는 것: namespace/XML 방문 오류를 무시하여 script 증명 생성.
#[test]
fn script_namespace_and_xml_refused() {
    let good = script_input();
    for (part, from, to) in [
        ("Contents/content.hpf", "http://www.idpf.org/2007/opf/", "urn:other"),
        ("META-INF/container.xml", "urn:oasis:names:tc:opendocument:xmlns:container", "urn:other"),
        (
            "Contents/content.hpf",
            "<opf:itemref idref='aux-h'/>",
            "<unbound:itemref idref='aux-h'/>",
        ),
        ("Contents/content.hpf", "</opf:package>", ""),
        ("META-INF/container.xml", "</ocf:container>", ""),
        ("Contents/content.hpf", "</opf:package>", "</opf:package><opf:package/>"),
        ("META-INF/container.xml", "</ocf:container>", "</ocf:container><ocf:container/>"),
        ("Contents/content.hpf", "</opf:manifest>", "</opf:manifest><opf:manifest/>"),
        ("Contents/content.hpf", "</opf:spine>", "</opf:spine><opf:spine/>"),
        ("Contents/content.hpf", "<opf:package ", "<!DOCTYPE package><opf:package "),
        ("META-INF/container.xml", "<ocf:container ", "<!DOCTYPE container><ocf:container "),
    ] {
        let input = replace(&good, part, from, to);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&input, &[TARGET]).unwrap_err(),
            original_error(&input)
        );
        if let Ok(d) = HwpxDecoder::decode(&input) {
            assert!(d.source_basis.as_ref().unwrap().losses.is_empty());
        }
    }
    let deep = format!("{}{}", "<x>".repeat(65), "</x>".repeat(65));
    no_script_evidence(&replace(&good, "Contents/content.hpf", "<opf:title/>", &deep), TARGET);
}

// 이것을 실패시키는 것: 1:1 소비의 SHA/index/kind/part/section/잔여 근거 검사 생략.
#[test]
fn script_evidence_is_one_to_one() {
    use crate::decoder::preserved_loss::LossLocation;
    let base = scalar(&script_input());
    for change in
        ["missing", "duplicate", "extra", "index", "kind", "part", "sections", "order", "sha"]
    {
        let (mut d, p, _) = admit_delete(&base).unwrap();
        let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
        assert_eq!(basis.losses.len(), 2);
        match change {
            "missing" => {
                basis.losses.pop();
            }
            "duplicate" => {
                basis.losses[1] = basis.losses[0].clone();
            }
            "extra" => basis.losses.push(basis.losses[1].clone()),
            "index" => basis.losses[1].warning = 99,
            "kind" => {
                let crate::DecodeWarning::ContentLost { kind, .. } =
                    &mut d.warnings[basis.losses[1].warning]
                else {
                    panic!()
                };
                *kind = "unknown".into();
            }
            "part" => {
                let crate::DecodeWarning::ContentLost { part, .. } =
                    &mut d.warnings[basis.losses[1].warning]
                else {
                    panic!()
                };
                *part = "Contents/section0.xml".into();
            }
            "sections" => {
                let LossLocation::Package { sections } = &mut basis.losses[1].location else {
                    panic!()
                };
                sections.swap(0, 1);
            }
            "order" => basis.losses.swap(0, 1),
            "sha" => {}
            _ => unreachable!(),
        }
        let different = script_input();
        let input = match change {
            "sha" => &different,
            _ => &base,
        };
        assert!(preserved_loss::check_candidates(input, &p, &d).is_err(), "{change}");
    }
}

// 이것을 실패시키는 것: reason 문자열 분류 또는 script 문구로 무근거 손실 허용.
#[test]
fn script_warning_text_is_not_policy() {
    let base = scalar(&script_input());
    let (mut d, p, _) = admit_delete(&base).unwrap();
    let expected = d.warnings.clone();
    assert_eq!(
        d.source_basis.as_ref().unwrap().losses.iter().map(|l| l.warning).collect::<Vec<_>>(),
        [0, 1]
    );
    for w in &mut d.warnings {
        assert!(w.is_content_loss());
        let crate::DecodeWarning::ContentLost { reason, .. } = w else { panic!() };
        *reason = "unrelated opaque diagnostic".into();
    }
    preserved_loss::check_candidates(&base, &p, &d).unwrap();
    let crate::DecodeWarning::ContentLost { reason, .. } = &expected[1] else { panic!() };
    d.warnings.push(crate::DecodeWarning::ContentLost {
        part: "Contents/section0.xml".into(),
        path: Default::default(),
        kind: "unknown_element".into(),
        reason: reason.clone(),
    });
    assert!(preserved_loss::check_candidates(&base, &p, &d).is_err());
}

// 이것을 실패시키는 것: 최종 package payload 또는 승인 범위 밖 source 대조 생략.
#[test]
fn script_final_payload_corruption_refused() {
    let base = scalar(&script_input());
    let d = HwpxDecoder::decode(&base).unwrap();
    let output = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    let mut expected = d.document.clone();
    expected.sections_mut()[0].paragraphs.remove(1);
    let changes = crate::raw_package::OutputChanges {
        rewritten: vec!["Contents/section0.xml".into()],
        ..Default::default()
    };
    for (part, from, to) in [
        ("Scripts/headerScripts.js", "/\0/\0h\0", "/\0/\0x\0"),
        (
            "Contents/content.hpf",
            "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>",
            "<opf:itemref idref='aux-s'/><opf:itemref idref='aux-h'/>",
        ),
        ("Contents/header.xml", "latin=\"91\"", "latin=\"87\""),
        ("Contents/section0.xml", "outlineShapeIDRef=\"1\"", "outlineShapeIDRef=\"0\""),
    ] {
        let altered = replace(&output, part, from, to);
        let change = SectionChange {
            section: 0,
            path: "Contents/section0.xml".into(),
            original: xml(&base),
            deleted: vec![1],
            inserted: None,
            ids: ParagraphIds::Preserve,
            heights: vec![],
        };
        assert!(
            finish_edit(
                &base,
                RawPackage::read(&altered).unwrap(),
                &d,
                expected.clone(),
                &changes,
                &[change]
            )
            .is_err(),
            "{part}"
        );
    }
}

// 이것을 실패시키는 것: Preview 제거 호출 생략.
#[test]
fn script_preview_removal_is_bounded() {
    let base = extra(
        &extra(&script_input(), "Preview/PrvText.txt", b"preview"),
        "Preview/PrvImage.png",
        b"image",
    );
    let item = "<opf:item id='prv' href='Preview/PrvText.txt' media-type='text/plain'/>";
    let root = "<ocf:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/>";
    let base = replace(
        &base,
        "Contents/content.hpf",
        "</opf:manifest>",
        &format!("{item}</opf:manifest>"),
    );
    let base = replace(
        &base,
        "META-INF/container.xml",
        "</ocf:rootfiles>",
        &format!("{root}</ocf:rootfiles>"),
    );
    let out =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET]).unwrap();
    assert_eq!(out.encode_warnings.len(), 1);
    assert_eq!(out.encode_warnings[0].code(), "PREVIEW_REMOVED");
    let before = RawPackage::read(&base).unwrap();
    let after = RawPackage::read(&out.value).unwrap();
    assert!(!after.entry_paths().any(|p| p.starts_with("Preview/")));
    for (part, removed) in [("Contents/content.hpf", item), ("META-INF/container.xml", root)] {
        assert_eq!(
            after.read_text_entry(part).unwrap(),
            before.read_text_entry(part).unwrap().replace(removed, "")
        );
    }
    for part in [
        "Contents/header.xml",
        "Scripts/headerScripts.js",
        "Scripts/sourceScripts.js",
        "Contents/section1.xml",
    ] {
        assert_eq!(before.read_entry(part).unwrap(), after.read_entry(part).unwrap());
    }
}

// 이것을 실패시키는 것: script 근거가 표 높이/줄 캐시 범위 gate를 우회.
#[test]
fn script_tables_and_caches_remain_scoped() {
    let base = scripts(&table_base(), "<opf:itemref idref='aux-h'/>");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    let source = xml(&out);
    assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 1);
    assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 1);
    assert_eq!(source.matches(CACHE).count(), 3);
    let mut package = RawPackage::read(&base).unwrap();
    package.replace_text_entry(
        "Contents/section0.xml",
        xml(&base).replace("treatAsChar=\"0\"", "treatAsChar=\"1\""),
    );
    let bad = package.write().unwrap();
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&bad, &[TARGET]).unwrap_err(),
        original_error(&bad)
    );
}

// 이것을 실패시키는 것: preserving 내부 실패 노출 또는 no-op에 admission 적용.
#[test]
fn script_original_admission_error_and_noop() {
    let base = script_input();
    let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[]).unwrap();
    assert_eq!(out.value, base);
    assert!(out.decode_warnings.is_empty());
    assert!(out.encode_warnings.is_empty());
    let expected = original_error(&base);
    for gate in [
        "header",
        "plain-source",
        "derived-heights",
        "package-final",
        "source-final",
        "preview-final",
    ] {
        failures(&[gate]);
        let actual = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err();
        assert_eq!(actual, expected);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(actual.to_string(), expected.to_string());
        assert!(visits().contains(&gate));
    }
    failures(&[]);
}

// Only these exact manifest/spine additions differ; Scripts payloads stay in both inputs.
fn without_script_order_items(content: &str) -> String {
    content
        .replace("<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>", "")
        .replace("<opf:item id='aux-h' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
        .replace("<opf:item id='aux-s' href='Scripts/sourceScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
}

fn assert_script_editor_result<T, M, E>(
    api: &str,
    input: &[u8],
    control_input: &[u8],
    actual: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    control: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    view: impl for<'a> Fn(&'a T, &[u8]) -> (&'a [u8], M),
) -> bool
where
    T: std::fmt::Debug,
    M: std::fmt::Debug + PartialEq,
    E: std::fmt::Debug + std::fmt::Display,
{
    match (actual, control) {
        (Ok(actual), Ok(control)) => {
            // The input package warning is intentional; each editor must retain it exactly.
            assert_eq!(
                actual.decode_warnings,
                HwpxDecoder::decode(input).unwrap().warnings,
                "{api}: script decode warnings"
            );
            assert_eq!(
                control.decode_warnings,
                HwpxDecoder::decode(control_input).unwrap().warnings,
                "{api}: control decode warnings"
            );
            assert_eq!(actual.encode_warnings, control.encode_warnings, "{api}: encode warnings");
            let (actual_bytes, actual_metadata) = view(&actual.value, input);
            let (control_bytes, control_metadata) = view(&control.value, control_input);
            assert_eq!(actual_metadata, control_metadata, "{api}: result metadata");
            let actual_package = RawPackage::read(actual_bytes).unwrap();
            let control_package = RawPackage::read(control_bytes).unwrap();
            assert_eq!(
                actual_package.entry_paths().collect::<Vec<_>>(),
                control_package.entry_paths().collect::<Vec<_>>(),
                "{api}: entries"
            );
            for path in actual_package.entry_paths() {
                let actual_entry = actual_package.read_entry(path).unwrap();
                let control_entry = control_package.read_entry(path).unwrap();
                if path == "Contents/content.hpf" {
                    assert_eq!(
                        actual_entry,
                        RawPackage::read(input).unwrap().read_entry(path).unwrap(),
                        "{api}: script manifest preservation"
                    );
                    assert_eq!(
                        without_script_order_items(std::str::from_utf8(actual_entry).unwrap()),
                        std::str::from_utf8(control_entry).unwrap(),
                        "{api}: {path}"
                    );
                } else {
                    // Includes edited section XML, header and every preserved opaque payload.
                    assert_eq!(actual_entry, control_entry, "{api}: {path}");
                }
                if matches!(path, "Scripts/headerScripts.js" | "Scripts/sourceScripts.js") {
                    assert_eq!(
                        actual_entry,
                        RawPackage::read(input).unwrap().read_entry(path).unwrap(),
                        "{api}: {path} preservation"
                    );
                    assert_eq!(
                        control_entry,
                        RawPackage::read(control_input).unwrap().read_entry(path).unwrap(),
                        "{api}: {path} control preservation"
                    );
                }
            }
            true
        }
        (Err(actual), Err(control)) => {
            assert_eq!(std::mem::discriminant(&actual), std::mem::discriminant(&control), "{api}");
            assert_eq!(actual.to_string(), control.to_string(), "{api}");
            assert_eq!(format!("{actual:?}"), format!("{control:?}"), "{api}: error fields");
            false
        }
        (actual, control) => panic!("{api}: script={actual:?}, control={control:?}"),
    }
}

// 이것을 실패시키는 것: 성공을 유지한 script patch의 편집 XML/보존 payload 훼손 또는 decode/encode 경고 삭제.
#[test]
fn script_other_editors_and_reencode_unchanged() {
    let seed = generated();
    let mut doc = HwpxDecoder::decode(&seed).unwrap().document;
    doc.sections_mut()[0].paragraphs.push(Paragraph::with_runs(
        vec![Run::control(
            hwpforge_core::Control::Field {
                field_type: hwpforge_foundation::FieldType::ClickHere,
                hint_text: Some("값".into()),
                help_text: None,
                name: Some("field".into()),
                display_text: String::new(),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    ));
    let field = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();

    let mut patch_successes = 0;
    for base in [
        seed.clone(),
        scalar(&seed),
        inline(&seed, "남음"),
        body_object(&seed, "남음", &shape()),
        body_object(
            &seed,
            "남음",
            &object(
                "container",
                1000,
                1000,
                "1",
                &picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#),
            ),
        ),
        header_order(&seed),
        field,
    ] {
        let input = scripts(&base, "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>");
        // Keep the same ZIP payload so closed-world admission has the same input inventory.
        let mut control = RawPackage::read(&input).unwrap();
        let content = control.read_text_entry("Contents/content.hpf").unwrap();
        let content = without_script_order_items(&content);
        control.replace_text_entry("Contents/content.hpf", content);
        let control = control.write().unwrap();
        let d = HwpxDecoder::decode(&input).unwrap();
        let c = HwpxDecoder::decode(&control).unwrap();
        assert_eq!(d.document, c.document);
        assert_eq!(d.style_store, c.style_store);
        assert_eq!(d.image_store, c.image_store);
        let is_package = |w: &&crate::DecodeWarning| {
            matches!(w,
            crate::DecodeWarning::ContentLost { kind, .. } if kind == "package_reading_order")
        };
        assert_eq!(d.warnings.iter().filter(is_package).count(), 1);
        let control_package = c.warnings.iter().filter(is_package).count();
        assert_eq!(control_package, usize::from(base == header_order(&seed)));
        assert_eq!(
            d.warnings.iter().filter(|w| !is_package(w)).collect::<Vec<_>>(),
            c.warnings.iter().filter(|w| !is_package(w)).collect::<Vec<_>>()
        );
        if control_package == 1 {
            assert_eq!(d.warnings, c.warnings);
        } else {
            assert_eq!(d.warnings.len(), c.warnings.len() + 1);
        }
        let reencode = |decoded: &crate::HwpxDocument| {
            crate::HwpxEncoder::encode_with_diagnostics(
                &decoded.document.clone().validate().unwrap(),
                &decoded.style_store,
                &decoded.image_store,
                crate::EncodeOptions::default(),
            )
            .unwrap()
        };
        let encoded = reencode(&d);
        let encoded_control = reencode(&c);
        assert_eq!(encoded.bytes, encoded_control.bytes);
        assert_eq!(encoded.warnings, encoded_control.warnings);
        assert_script_editor_result(
            "stamp",
            &input,
            &control,
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&input, &[]),
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&control, &[]),
            |result, source| {
                // Whole-ZIP hashes differ with the intended input differences; verify each directly.
                assert_eq!(result.manifest.source_sha256, crate::patch::sha256_hex(source));
                assert_eq!(result.manifest.output_sha256, crate::patch::sha256_hex(&result.bytes));
                (
                    &result.bytes,
                    (
                        result.manifest.schema_version,
                        serde_json::to_value(&result.manifest.fields).unwrap(),
                        result.outcome.clone(),
                    ),
                )
            },
        );
        let patch = |bytes: &[u8], decoded: &crate::HwpxDocument| {
            let mut replacement = decoded.document.sections()[0].clone();
            replacement.paragraphs[1] = paragraph("수정\u{1}");
            let preservation = crate::HwpxPatcher::export_section_preservation(
                bytes,
                0,
                &decoded.document.sections()[0],
            )
            .unwrap();
            crate::HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
                bytes,
                0,
                &replacement,
                None,
                Some(&preservation),
            )
        };
        patch_successes += usize::from(assert_script_editor_result(
            "patch",
            &input,
            &control,
            patch(&input, &d),
            patch(&control, &c),
            |bytes, _| (bytes, ()),
        ));
        let fills = [("field".into(), "채움".into())].into();
        assert_script_editor_result(
            "fill",
            &input,
            &control,
            crate::HwpxFiller::fill_with_codec_diagnostics(&input, &fills),
            crate::HwpxFiller::fill_with_codec_diagnostics(&control, &fills),
            |result, _| (&result.bytes, serde_json::to_value(&result.filled).unwrap()),
        );
        assert_script_editor_result(
            "insert",
            &input,
            &control,
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &input,
                TARGET,
                InsertPosition::After,
                &["삽입".into()],
            ),
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &control,
                TARGET,
                InsertPosition::After,
                &["삽입".into()],
            ),
            |bytes, _| (bytes, ()),
        );
    }
    assert!(patch_successes > 0, "must exercise successful editor payload comparisons");
}

// 이것을 실패시키는 것: 허용 PAGE 삭제를 거부로 변경하거나 높이 검사 실패를 승인.
#[test]
fn script_derived_inputs_stay_the_same() {
    let number = |kind| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='DIGIT' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    #[derive(Clone, Copy, Debug)]
    enum Expect {
        Allow,
        RejectDerived,
        RejectHeight,
    }
    let mut cases = vec![
        (number("PAGE"), Expect::Allow),
        (number("TOTAL_PAGE"), Expect::Allow),
        (number("UNKNOWN"), Expect::RejectDerived),
        (number("PAGE").replace("type='DIGIT'", "type='UNKNOWN'"), Expect::RejectDerived),
        (format!("<hp:ctrl><hp:footer><hp:subList><hp:p><hp:run>{}</hp:run></hp:p></hp:subList></hp:footer></hp:ctrl>", number("TOTAL_PAGE")), Expect::RejectDerived),
    ];
    for policy in ["CONTINUOUS", "ON_SECTION", "ON_PAGE", "UNKNOWN"] {
        cases.push((format!("<hp:footNotePr><hp:numbering type='{policy}'/></hp:footNotePr><hp:ctrl><hp:footNote number='1'><hp:subList><hp:p><hp:run><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>"), Expect::RejectDerived));
    }
    for (kind, expect) in [
        ("OBJECT_TYPE_PAGE", Expect::Allow),
        ("OBJECT_TYPE_NUMBER", Expect::RejectDerived),
        ("OBJECT_TYPE_CONTENTS", Expect::RejectDerived),
    ] {
        cases.push((format!("<hp:ctrl><hp:bookmark name='mark'/><hp:fieldBegin id='10' type='CROSSREF' dirty='0'><hp:parameters><hp:stringParam name='RefType'>TARGET_BOOKMARK</hp:stringParam><hp:stringParam name='RefContentType'>{kind}</hp:stringParam><hp:stringParam name='RefPath'>?mark;</hp:stringParam><hp:stringParam name='Command'>?mark;6;0;0;0;</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl><hp:t>3</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>"), expect));
    }
    let mut cases: Vec<_> = cases
        .into_iter()
        .map(|(content, expect)| {
            let base =
                replace(&script_seed(), "Contents/section0.xml", "<hp:t>첫 생존</hp:t>", &content);
            (content, base, expect)
        })
        .collect();
    let mut table = RawPackage::read(&script_seed()).unwrap();
    table.replace_text_entry(SECTION, xml(&table_base()));
    let table = table.write().unwrap();
    cases.push(("supported table height".into(), table.clone(), Expect::Allow));
    let mut unsupported_table = RawPackage::read(&table).unwrap();
    unsupported_table
        .replace_text_entry(SECTION, xml(&table).replace("treatAsChar=\"0\"", "treatAsChar=\"1\""));
    cases.push((
        "unsupported inline table height".into(),
        unsupported_table.write().unwrap(),
        Expect::RejectHeight,
    ));
    for (content, base, expect) in cases {
        // Both inputs must exercise preserving deletion, where the derived gates apply.
        let base = extra(&base, "Scripts/opaque.bin", b"unrelated payload");
        let decision = |input: &[u8]| {
            let d = HwpxDecoder::decode(input).unwrap();
            let p = RawPackage::read(input).unwrap();
            let f = crate::derived_values::DerivedFacts::collect(&p, &d).unwrap();
            let ordinals = [f.body_ordinal(0, 1).unwrap()];
            let effects = f
                .effects(&[crate::derived_values::DerivedEdit::Delete {
                    section: 0,
                    paragraphs: &ordinals,
                }])
                .unwrap();
            (
                effects.check_derived_values().map_err(|e| e.to_string()),
                effects.collect_height_invalidations().map_err(|e| e.to_string()),
            )
        };
        let expected = decision(&base);
        let expected_checks = match expect {
            Expect::Allow => (true, true),
            Expect::RejectDerived => (false, true),
            Expect::RejectHeight => (true, false),
        };
        assert_eq!((expected.0.is_ok(), expected.1.is_ok()), expected_checks, "{content}");
        let assert_delete = |input: &[u8]| {
            let result = HwpxStructuralEditor::delete_paragraphs(input, &[TARGET]);
            match expect {
                Expect::Allow => {
                    let output = result
                        .unwrap_or_else(|e| panic!("{content}: expected allowed deletion: {e}"));
                    let before = HwpxDecoder::decode(input).unwrap();
                    let mut expected_document = before.document;
                    expected_document.sections_mut()[0].paragraphs.remove(TARGET.index);
                    let mut actual_document = HwpxDecoder::decode(&output).unwrap().document;
                    // Successful deletion invalidates stored heights and paragraph line caches.
                    expected_document.strip_layout_caches();
                    actual_document.strip_layout_caches();
                    assert_eq!(actual_document, expected_document, "{content}");
                }
                Expect::RejectDerived | Expect::RejectHeight => {
                    let error = match result {
                        Err(error) => error,
                        Ok(_) => panic!("{content}: expected refused deletion"),
                    };
                    assert_eq!(error, original_error(input), "{content}");
                }
            }
        };
        assert_delete(&base);
        for anchor in ["<opf:spine>", "<opf:itemref idref=\"body-b\"", "</opf:spine>"] {
            let input = scripts(&base, "");
            let inserted = "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>";
            let input =
                replace(&input, "Contents/content.hpf", anchor, &format!("{inserted}{anchor}"));
            // For the opening tag, insert inside rather than before the spine.
            let input = match anchor {
                "<opf:spine>" => replace(
                    &input,
                    "Contents/content.hpf",
                    &format!("{inserted}{anchor}"),
                    &format!("{anchor}{inserted}"),
                ),
                _ => input,
            };
            assert_eq!(decision(&input), expected, "{content}");
            assert_delete(&input);
        }
    }
}

// 이것을 실패시키는 것: 근거마다 전체 탐색 반복 또는 facts 중복 수집.
#[test]
fn script_work_is_bounded() {
    let measure = |n: usize, tables: usize| {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            std::iter::once(paragraph("시작"))
                .chain(std::iter::once(paragraph("삭제")))
                .chain((0..n).map(|_| paragraph("남음")))
                .chain((0..tables).map(|_| table_host()))
                .collect(),
            PageSettings::a4(),
        ));
        let base = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let mut p = RawPackage::read(&base).unwrap();
        p.replace_text_entry(
            "Contents/section0.xml",
            xml(&base).replace("<hp:t>남음", "<hp:t charStyleIDRef='0'>남음"),
        );
        let base = scripts(
            &p.write().unwrap(),
            "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>",
        );
        crate::derived_values::COLLECTIONS.with(|c| c.set(0));
        let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[TARGET])
            .unwrap();
        assert_eq!(crate::derived_values::COLLECTIONS.with(std::cell::Cell::get), 1);
        assert_eq!(out.decode_warnings.iter().filter(|w| w.is_content_loss()).count(), n + 1);
        let d = HwpxDecoder::decode(&base).unwrap();
        let p = RawPackage::read(&base).unwrap();
        let f = crate::derived_values::DerivedFacts::collect(&p, &d).unwrap();
        let ordinals = [f.body_ordinal(0, 1).unwrap()];
        crate::work::WORK.with(|w| w.set(0));
        let effects = f
            .effects(&[crate::derived_values::DerivedEdit::Delete {
                section: 0,
                paragraphs: &ordinals,
            }])
            .unwrap();
        preserved_loss::check_candidates(&base, &p, &d).unwrap();
        preserved_loss::check_locations(&d, &effects).unwrap();
        crate::work::WORK.with(std::cell::Cell::get)
    };
    for tables in [0, 160] {
        let small = measure(160, tables);
        let large = measure(320, tables * 2);
        assert!(small >= 160);
        assert!(large > small);
        assert!(large <= small * 2 + 64);
    }
}

const HEADER: &str = "Contents/header.xml";
fn scalar(base: &[u8]) -> Vec<u8> {
    replace(
        base,
        HEADER,
        "<hh:ratio hangul=\"100\" latin=\"100\"",
        "<hh:ratio hangul=\"100\" latin=\"91\"",
    )
}
fn inline(base: &[u8], text: &str) -> Vec<u8> {
    replace(base, SECTION, &format!("<hp:t>{text}"), &format!("<hp:t charStyleIDRef='0'>{text}"))
}
fn header_order(base: &[u8]) -> Vec<u8> {
    replace(
        base,
        "Contents/content.hpf",
        "idref=\"header\" linear=\"yes\"",
        "idref=\"header\" linear=\"no\"",
    )
}
fn succeeds(base: &[u8]) {
    let decoded = HwpxDecoder::decode(base).unwrap();
    assert!(decoded.warnings.iter().any(crate::DecodeWarning::is_content_loss));
    let out =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(base, &[TARGET]).unwrap();
    assert_eq!(out.decode_warnings, decoded.warnings);
    let before = RawPackage::read(base).unwrap();
    let after = RawPackage::read(&out.value).unwrap();
    for p in before.entry_paths().filter(|p| *p != SECTION) {
        assert_eq!(before.read_entry(p).unwrap(), after.read_entry(p).unwrap(), "{p}");
    }
}

// 이것을 실패시키는 것: header scalar를 body paragraph0 손실로 취급하거나 전역 거부 유지.
#[test]
fn scalar_header_survives_with_original_warning() {
    let base = scalar(&carry_base());
    succeeds(&base);
    succeeds(&header_order(&base));
    let base = replace(&base, SECTION, "<hp:t>가", "<hp:t>다른 사용 위치 가");
    succeeds(&base);
    // A plain paragraph zero has no secPr; header's default path must not become its address.
    let d = HwpxDecoder::decode(&base).unwrap();
    let source = xml(&base);
    let first = &source[d.source_maps[0].paragraphs[0].outer.clone()];
    let plain = r#"<hp:p id="0" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0"><hp:t>첫 문단</hp:t></hp:run></hp:p>"#;
    let base = replace(&base, SECTION, first, plain);
    let before = HwpxDecoder::decode(&base).unwrap();
    let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(
        &base,
        &[ParagraphLocator { section: 0, index: 0 }],
    )
    .unwrap();
    assert_eq!(out.decode_warnings, before.warnings);
    assert_eq!(
        RawPackage::read(&out.value).unwrap().read_entry(HEADER).unwrap(),
        RawPackage::read(&base).unwrap().read_entry(HEADER).unwrap()
    );
}

// 이것을 실패시키는 것: inline style 전역 허용 또는 생존 위치도 전역 거부.
#[test]
fn inline_style_must_survive_the_deletion() {
    succeeds(&inline(&carry_base(), "남음"));
    let base = inline(&carry_base(), "가");
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
        original_error(&base)
    );
}

// 이것을 실패시키는 것: header-only 비선형 순서 근거 누락 또는 section 비선형까지 허용.
#[test]
fn nonlinear_order_requires_only_the_header() {
    succeeds(&header_order(&carry_base()));
    let base = replace(
        &carry_base(),
        "Contents/content.hpf",
        "idref=\"section0\" linear=\"yes\"",
        "idref=\"section0\" linear=\"no\"",
    );
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
        original_error(&base)
    );
}

// 이것을 실패시키는 것: metadata 파싱 실패를 무시하고 package 순서 근거를 생성.
#[test]
fn nonlinear_order_with_unreadable_metadata_is_refused() {
    let plain = carry_base();
    for input in [
        plain.clone(),
        scalar(&plain),
        inline(&plain, "남음"),
        body_object(&plain, "남음", &shape()),
        body_object(&plain, "남음", &picture("2", "")),
        body_object(&plain, "남음", &group(&object("rect", 500, 500, "1", ""))),
        body_object(
            &plain,
            "남음",
            &object(
                "container",
                1000,
                1000,
                "1",
                &picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#),
            ),
        ),
    ] {
        let base = header_order(&input);
        succeeds(&base);
        let broken = replace(
            &base,
            "Contents/content.hpf",
            "<opf:title/>",
            "<opf:title><![CDATA[x]]></opf:title>",
        );
        let decoded = HwpxDecoder::decode(&broken).unwrap();
        assert_eq!(decoded.warnings, HwpxDecoder::decode(&base).unwrap().warnings);
        assert!(!decoded.source_basis.as_ref().unwrap().losses.iter().any(|loss| {
            loss.cause == crate::decoder::preserved_loss::LossCause::HeaderOnlyNonlinearOrder
        }));
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&broken, &[TARGET]).unwrap_err(),
            original_error(&broken)
        );
    }
}

fn matrix(scale: &str) -> String {
    format!(
        r#"<hp:renderingInfo><hc:transMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/><hc:scaMatrix e1="{scale}" e2="0" e3="0" e4="0" e5="{scale}" e6="0"/><hc:rotMatrix e1="1" e2="0" e3="0" e4="0" e5="1" e6="0"/></hp:renderingInfo>"#
    )
}
fn object(tag: &str, org: i32, shown: i32, scale: &str, tail: &str) -> String {
    format!(
        r#"<hp:{tag} id="9" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM" textFlow="BOTH_SIDES" lock="0" dropcapstyle="None" href="" groupLevel="0" instid="7"><hp:offset x="0" y="0"/><hp:orgSz width="{org}" height="{org}"/><hp:curSz width="{shown}" height="{shown}"/>{}<hp:sz width="{shown}" widthRelTo="ABSOLUTE" height="{shown}" heightRelTo="ABSOLUTE" protect="0"/>{tail}</hp:{tag}>"#,
        matrix(scale)
    )
}
fn shape() -> String {
    object("rect", 1000, 1000, "2", "")
}
fn picture(scale: &str, tail: &str) -> String {
    object(
        "pic",
        1000,
        1000,
        scale,
        &format!(
            r#"<hc:img binaryItemIDRef="" effect="REAL_PIC" bright="0" contrast="0" alpha="0"/>{tail}"#
        ),
    )
}
fn group(tail: &str) -> String {
    object("container", 1000, 800, "1", tail)
}
fn body_object(base: &[u8], text: &str, object: &str) -> Vec<u8> {
    replace(base, SECTION, &format!("<hp:t>{text}</hp:t>"), object)
}
fn assert_cause(base: &[u8], kind: &str) {
    let d = HwpxDecoder::decode(base).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| matches!(w, crate::DecodeWarning::ContentLost {kind:k,..} if k==kind)),
        "{:?}",
        d.warnings
    );
}

// 이것을 실패시키는 것: 일반 도형·그림·group residual 어느 생성 지점의 근거든 누락/잘못된 인덱스.
#[test]
fn all_drawn_extent_sites_preserve_surviving_objects() {
    for object in [shape(), picture("2", ""), group(&object("rect", 500, 500, "1", ""))] {
        let base = body_object(&carry_base(), "남음", &object);
        assert_cause(&base, "shape_drawn_extent");
        succeeds(&base);
        succeeds(&scalar(&base));
        let base = body_object(&carry_base(), "가 &amp; &lt;나&gt;끝", &object);
        assert_cause(&base, "shape_drawn_extent");
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
            original_error(&base)
        );
    }
}

// 이것을 실패시키는 것: 같은 group_child_picture kind를 회전·뒤집기까지 허용하거나 appearance 근거 누락.
#[test]
fn group_picture_appearance_and_transform_have_different_causes() {
    for pic in [
        picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#),
        picture("1", r#"<hp:effects><hp:glow/></hp:effects>"#),
        picture("1", "").replace("bright=\"0\"", "bright=\"10\""),
    ] {
        let base = body_object(&carry_base(), "남음", &object("container", 1000, 1000, "1", &pic));
        assert_cause(&base, "group_child_picture");
        succeeds(&base);
        let retried = body_object(&carry_base(), "남음", &group(&pic.replace("1000", "800")));
        succeeds(&retried);
    }
    for transform in [
        matrix("1").replace(
            "<hc:rotMatrix e1=\"1\" e2=\"0\" e3=\"0\" e4=\"0\" e5=\"1\"",
            "<hc:rotMatrix e1=\"0\" e2=\"-1\" e3=\"0\" e4=\"1\" e5=\"0\"",
        ),
        matrix("1").replace("<hc:scaMatrix e1=\"1\"", "<hc:scaMatrix e1=\"-1\""),
    ] {
        let pic = picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#)
            .replace(&matrix("1"), &transform);
        let base = body_object(&carry_base(), "남음", &object("container", 1000, 1000, "1", &pic));
        assert_cause(&base, "group_child_picture");
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
            original_error(&base)
        );
    }
}

// 이것을 실패시키는 것: 영향 표 안 손실 허용, 삭제 위치만으로 전역 거부, 다른 section 주소 혼동.
#[test]
fn body_loss_locations_use_surviving_source_and_affected_tables() {
    let base = table_base();
    // The first table precedes the deleted body paragraph; the second follows it.
    let earlier = body_object(&base, "칸", &shape());
    succeeds(&earlier);
    let mut pkg = RawPackage::read(&base).unwrap();
    let raw = xml(&base);
    let at = raw.rfind("<hp:t>칸</hp:t>").unwrap();
    let mut changed = raw.clone();
    changed.replace_range(at..at + "<hp:t>칸</hp:t>".len(), &shape());
    pkg.replace_text_entry(SECTION, changed);
    let later = pkg.write().unwrap();
    assert_cause(&later, "shape_drawn_extent");
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&later, &[TARGET]).unwrap_err(),
        original_error(&later)
    );
    let mut doc = Document::new();
    for _ in 0..2 {
        doc.add_section(Section::with_paragraphs(
            vec![paragraph("처음"), paragraph("삭제"), paragraph("남음")],
            PageSettings::a4(),
        ));
    }
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = extra(&base, "Scripts/opaque.bin", b"carry");
    let base = replace(&base, "Contents/section1.xml", "<hp:t>남음</hp:t>", &shape());
    succeeds(&base);
}

// 이것을 실패시키는 것: reason/Display 문자열로 허용 판단, header default path를 body 위치로 취급.
#[test]
fn warning_text_and_header_default_path_do_not_authorize_loss() {
    let prefixed = replace(
        &scalar(&carry_base()),
        HEADER,
        "<hh:beginNum page=\"1\"",
        "<hh:beginNum page=\"0\"",
    );
    let decoded = HwpxDecoder::decode(&prefixed).unwrap();
    let evidence = &decoded.source_basis.as_ref().unwrap().losses[0];
    assert!(
        matches!(&decoded.warnings[evidence.warning],crate::DecodeWarning::ContentLost{kind,..} if kind==evidence.cause.kind())
    );

    for base in [scalar(&carry_base()), inline(&carry_base(), "남음"), header_order(&carry_base())]
    {
        let (mut d, p, error) = admit_delete(&base).unwrap();
        assert!(error.is_some());
        for warning in &mut d.warnings {
            if let crate::DecodeWarning::ContentLost { reason, path, .. } = warning {
                *reason = "opaque replacement without diagnostic keywords".into();
                *path = crate::decoder::ParagraphPath(vec![
                    crate::decoder::PathSeg::Section(99),
                    crate::decoder::PathSeg::BodyParagraph(1),
                ]);
            }
        }
        let expected = d.warnings.clone();
        let out = delete_admitted(&base, &[TARGET], d, p, ParagraphIds::Preserve).unwrap();
        assert_eq!(out.decode_warnings, expected);
    }
}

// 이것을 실패시키는 것: 경고/근거 일대일 대응·종류·파트·SHA·유일한 원본 run 검사의 각 생략.
#[test]
fn incomplete_or_stale_private_evidence_is_refused() {
    use crate::decoder::preserved_loss::{LossCause, LossLocation};
    for change in [
        "missing",
        "duplicate",
        "extra-evidence",
        "warning-part",
        "relocate-part",
        "index",
        "kind",
        "cause",
        "part",
        "digest",
        "range",
        "unrecorded",
        "duplicate-node",
        "extra",
        "variant",
        "package",
    ] {
        let base = body_object(&carry_base(), "남음", &shape());
        let (mut d, p, _) = admit_delete(&base).unwrap();
        let mut basis = d.source_basis.take().unwrap();
        let evidence = &mut std::sync::Arc::make_mut(&mut basis).losses;
        match change {
            "missing" => evidence.clear(),
            "duplicate" => evidence.push(evidence[0].clone()),
            "extra-evidence" => {
                let mut extra = evidence[0].clone();
                extra.warning = usize::MAX;
                evidence.push(extra);
            }
            "warning-part" | "relocate-part" => {
                if let crate::DecodeWarning::ContentLost { part, .. } =
                    &mut d.warnings[evidence[0].warning]
                {
                    *part = "Contents/absent.xml".into();
                }
                if change == "relocate-part" {
                    if let LossLocation::Body { part, .. } = &mut evidence[0].location {
                        *part = "Contents/absent.xml".into();
                    }
                }
            }
            "index" => evidence[0].warning = usize::MAX,
            "kind" => {
                if let crate::DecodeWarning::ContentLost { kind, .. } =
                    &mut d.warnings[evidence[0].warning]
                {
                    *kind = "unknown_element".into()
                }
            }
            "cause" => {
                evidence[0].cause = LossCause::PerLanguageScalar;
                if let crate::DecodeWarning::ContentLost { kind, .. } =
                    &mut d.warnings[evidence[0].warning]
                {
                    *kind = "char_shape_per_language".into();
                }
            }
            "part" => {
                if let LossLocation::Body { part, .. } = &mut evidence[0].location {
                    *part = "Contents/missing.xml".into()
                }
            }
            "digest" => {
                if let LossLocation::Body { digest, .. } = &mut evidence[0].location {
                    digest[0] ^= 1
                }
            }
            "range" => {
                if let LossLocation::Body { span, .. } = &mut evidence[0].location {
                    span.end -= 1
                }
            }
            "unrecorded" => d.source_maps.clear(),
            "duplicate-node" => d.source_maps.push(d.source_maps[0].clone()),
            "extra" => d.warnings.push(crate::DecodeWarning::ContentLost {
                part: SECTION.into(),
                path: Default::default(),
                kind: "ole".into(),
                reason: "opaque".into(),
            }),
            "variant" => {
                d.warnings[evidence[0].warning] = crate::DecodeWarning::InvalidFieldBoundary {
                    part: SECTION.into(),
                    reason: "unrepresented field structure".into(),
                }
            }
            "package" => {}
            _ => unreachable!(),
        }
        d.source_basis = Some(basis);
        let input = if change == "package" { carry_base() } else { base.clone() };
        let original = HwpxDecoder::decode(&base).unwrap();
        let facts = crate::derived_values::DerivedFacts::collect(&p, &original).unwrap();
        let ordinals = [facts.body_ordinal(0, 1).unwrap()];
        let effects = facts
            .effects(&[crate::derived_values::DerivedEdit::Delete {
                section: 0,
                paragraphs: &ordinals,
            }])
            .unwrap();
        assert!(
            preserved_loss::check_candidates(&input, &p, &d)
                .and_then(|()| preserved_loss::check_locations(&d, &effects))
                .is_err(),
            "{change}"
        );
    }
}

// 이것을 실패시키는 것: group retry/slot 경고 이동에서 private 근거 인덱스의 재배치 생략.
#[test]
fn nested_group_warning_indices_follow_the_final_warning_order() {
    let inner = object(
        "container",
        1000,
        1000,
        "1",
        &picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#),
    );
    let text = format!(
        r#"<hp:drawText><hp:subList><hp:p id="7" paraPrIDRef="0" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0"><hp:run charPrIDRef="0">{inner}</hp:run></hp:p></hp:subList></hp:drawText>"#
    );
    let outer = group(&object("rect", 500, 500, "1", &text));
    let base = body_object(&carry_base(), "남음", &outer);
    assert_cause(&base, "group_child_picture");
    succeeds(&base);
}

// 이것을 실패시키는 것: header-only를 선택 속성·script·중복/누락 참조까지 확대.
#[test]
fn nonlinear_header_requires_the_original_resolver_proofs() {
    let good = header_order(&carry_base());
    for (part, from, to) in [
        ("Contents/content.hpf", "linear=\"no\"", "linear=\"maybe\""),
        ("Contents/content.hpf", "idref=\"header\"", "idref=\"header\" future=\"1\""),
        ("Contents/content.hpf", "<opf:spine>", "<opf:spine future=\"1\">"),
        ("Contents/content.hpf", "id=\"header\"", "id=\"header\" future=\"1\""),
        ("Contents/content.hpf", "</opf:spine>", "<opf:itemref idref=\"header\"/></opf:spine>"),
        ("Contents/content.hpf", "<opf:itemref idref=\"section0\" linear=\"yes\"/>", ""),
        ("META-INF/container.xml", "<ocf:rootfiles>", "<ocf:rootfiles future=\"1\">"),
    ] {
        let base = replace(&good, part, from, to);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
            original_error(&base),
            "{to}"
        );
    }
    let script = extra(&good, "Scripts/test.js", b"void 0;");
    let script = replace(
        &script,
        "Contents/content.hpf",
        "</opf:manifest>",
        r#"<opf:item id="script" href="Scripts/test.js" media-type="application/x-javascript"/></opf:manifest>"#,
    );
    let script = replace(
        &script,
        "Contents/content.hpf",
        "</opf:spine>",
        r#"<opf:itemref idref="script"/></opf:spine>"#,
    );
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&script, &[TARGET]).unwrap_err(),
        original_error(&script)
    );
}

// 이것을 실패시키는 것: header 정의 ID·SHA·파트 또는 검증된 package section 순서 검사 생략.
#[test]
fn header_and_package_evidence_cannot_be_relocated() {
    use crate::decoder::preserved_loss::LossLocation;
    for change in ["definition", "digest", "part", "sections", "package-part", "location", "absent"]
    {
        let base = if matches!(change, "sections" | "package-part") {
            header_order(&carry_base())
        } else {
            scalar(&carry_base())
        };
        let (mut d, p, _) = admit_delete(&base).unwrap();
        if change == "absent" {
            d.source_basis = None;
        } else {
            let evidence =
                &mut std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap()).losses[0];
            match change {
                "definition" => {
                    if let LossLocation::Header { definition, .. } = &mut evidence.location {
                        *definition = u32::MAX
                    }
                }
                "digest" => {
                    if let LossLocation::Header { digest, .. } = &mut evidence.location {
                        digest[0] ^= 1
                    }
                }
                "part" => {
                    if let crate::DecodeWarning::ContentLost { part, .. } =
                        &mut d.warnings[evidence.warning]
                    {
                        *part = SECTION.into()
                    }
                }
                "package-part" => {
                    if let crate::DecodeWarning::ContentLost { part, .. } =
                        &mut d.warnings[evidence.warning]
                    {
                        *part = SECTION.into();
                    }
                }
                "sections" => {
                    if let LossLocation::Package { sections } = &mut evidence.location {
                        sections.clear()
                    }
                }
                "location" => evidence.location = LossLocation::Package { sections: vec![] },
                _ => unreachable!(),
            }
        }
        assert!(preserved_loss::check_candidates(&base, &p, &d).is_err(), "{change}");
    }
}

// 이것을 실패시키는 것: 최종 package 검사 또는 section 원문 검사 생략으로 같은 decode 손실 뒤 변조를 숨김.
#[test]
fn final_checks_detect_corruption_hidden_by_the_same_decode_loss() {
    for change in ["header", "picture-border"] {
        let base = if change == "header" {
            scalar(&carry_base())
        } else {
            body_object(
                &carry_base(),
                "남음",
                &object(
                    "container",
                    1000,
                    1000,
                    "1",
                    &picture("1", r#"<hp:lineShape style="SOLID" width="42"/>"#),
                ),
            )
        };
        let d = HwpxDecoder::decode(&base).unwrap();
        let output = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
        let altered = if change == "header" {
            replace(&output, HEADER, "latin=\"91\"", "latin=\"87\"")
        } else {
            replace(&output, SECTION, "width=\"42\"", "width=\"141\"")
        };
        let mut expected = d.document.clone();
        expected.sections_mut()[0].paragraphs.remove(1);
        // Core equality alone deliberately cannot see the corrupted property.
        let expected_doc = HwpxDocument {
            document: expected.clone(),
            style_store: d.style_store.clone(),
            image_store: d.image_store.clone(),
            warnings: vec![],
            source_maps: vec![],
            source_basis: None,
        };
        self_verify(&altered, &expected_doc).unwrap();
        let changes = crate::raw_package::OutputChanges {
            rewritten: vec![SECTION.into()],
            ..Default::default()
        };
        let section_change = SectionChange {
            section: 0,
            path: SECTION.into(),
            original: xml(&base),
            deleted: vec![1],
            inserted: None,
            ids: ParagraphIds::Preserve,
            heights: vec![],
        };
        assert!(
            finish_edit(
                &base,
                RawPackage::read(&altered).unwrap(),
                &d,
                expected,
                &changes,
                &[section_change]
            )
            .is_err(),
            "{change}"
        );
    }
}

// 이것을 실패시키는 것: 좁은 면제를 no-op·기존 admission·insert로 확장하거나 한 번 수집 계약을 깨기.
#[test]
fn loss_scope_is_confined_to_preserving_delete() {
    let safe = generated();
    let (d, p, error) = admit_delete(&safe).unwrap();
    assert!(error.is_none());
    let before = delete_admitted(&safe, &[TARGET], d, p, ParagraphIds::Reindex).unwrap();
    let after =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&safe, &[TARGET]).unwrap();
    assert_eq!(before.value, after.value);
    assert_eq!(before.decode_warnings, after.decode_warnings);
    assert_eq!(before.encode_warnings, after.encode_warnings);
    let base = scalar(&carry_base());
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&base, &[]).unwrap().value,
        base
    );
    assert_eq!(
        HwpxStructuralEditor::insert_paragraphs(
            &base,
            TARGET,
            InsertPosition::After,
            &["삽입".into()]
        )
        .unwrap_err(),
        original_error(&base)
    );
    crate::derived_values::COLLECTIONS.with(|c| c.set(0));
    succeeds(&base);
    assert_eq!(crate::derived_values::COLLECTIONS.with(std::cell::Cell::get), 1);
}

// 이것을 실패시키는 것: effects에서 삭제된 원본 범위의 생존 검사 또는 정확한 원본 outer 검사 생략.
#[test]
fn source_loss_ranges_are_checked_against_original_effects() {
    use crate::decoder::preserved_loss::LossLocation;
    let base = body_object(&carry_base(), "가 &amp; &lt;나&gt;끝", &shape());
    let d = HwpxDecoder::decode(&base).unwrap();
    let p = RawPackage::read(&base).unwrap();
    let facts = crate::derived_values::DerivedFacts::collect(&p, &d).unwrap();
    let ordinals = [facts.body_ordinal(0, 1).unwrap()];
    let effects = facts
        .effects(&[crate::derived_values::DerivedEdit::Delete {
            section: 0,
            paragraphs: &ordinals,
        }])
        .unwrap();
    let LossLocation::Body { span, digest, .. } =
        &d.source_basis.as_ref().unwrap().losses[0].location
    else {
        panic!()
    };
    assert!(effects.check_preserved_loss(0, span, digest).is_err());
    let untouched = facts.effects(&[]).unwrap();
    untouched.check_preserved_loss(0, span, digest).unwrap();
    assert!(untouched.check_preserved_loss(0, &(span.start..span.end - 1), digest).is_err());
}

// 이것을 실패시키는 것: header 비선형 fallback의 section 순서 일치 검사 생략.
#[test]
fn nonlinear_header_does_not_authorize_reordered_sections() {
    let mut doc = Document::new();
    for text in ["첫 구역", "다음 구역"] {
        doc.add_section(Section::with_paragraphs(
            vec![paragraph("처음"), paragraph(text), paragraph("남음")],
            PageSettings::a4(),
        ));
    }
    let base = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let base = header_order(&extra(&base, "Scripts/opaque.bin", b"carry"));
    let base = replace(
        &base,
        "Contents/content.hpf",
        r#"<opf:itemref idref="section0" linear="yes"/><opf:itemref idref="section1" linear="yes"/>"#,
        r#"<opf:itemref idref="section1" linear="yes"/><opf:itemref idref="section0" linear="yes"/>"#,
    );
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap_err(),
        original_error(&base)
    );
}

// 이것을 실패시키는 것: 경고마다 근거 전체, 손실마다 SourceMap 전체 또는 영향 표 전체를 재검색.
// tick이 있는 순회만 센다. 정렬·이분 탐색 내부 비교와 계측 없는 순회는 세지 않으며,
// 전체 편집 시간의 선형성을 보장하지 않는다.
#[test]
fn preservation_evidence_work_scales_with_losses_and_source_nodes() {
    let measure = |n: usize, tables: usize| {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            std::iter::once(paragraph("처음"))
                .chain(std::iter::once(paragraph("삭제")))
                .chain((0..n).map(|_| paragraph("남음")))
                .chain((0..tables).map(|_| table_host()))
                .collect(),
            PageSettings::a4(),
        ));
        let base = crate::HwpxEncoder::encode(
            &doc.validate().unwrap(),
            &crate::style_store_for_preset("default").unwrap(),
            &ImageStore::new(),
        )
        .unwrap();
        let mut package = RawPackage::read(&base).unwrap();
        let source = package.read_text_entry(SECTION).unwrap();
        assert_eq!(source.matches("<hp:tbl ").count(), tables);
        package.replace_text_entry(
            SECTION,
            source.replace("<hp:t>남음", "<hp:t charStyleIDRef='0'>남음"),
        );
        let base = package.write().unwrap();
        let decoded = HwpxDecoder::decode(&base).unwrap();
        assert_eq!(decoded.source_basis.as_ref().unwrap().losses.len(), n);
        assert_eq!(decoded.warnings.iter().filter(|w| w.is_content_loss()).count(), n);
        let facts = crate::derived_values::DerivedFacts::collect(&package, &decoded).unwrap();
        let ordinals = [facts.body_ordinal(0, TARGET.index).unwrap()];
        // Exclude fixture creation and derived-facts collection; include the table index build.
        crate::work::WORK.with(|w| w.set(0));
        let effects = facts
            .effects(&[crate::derived_values::DerivedEdit::Delete {
                section: 0,
                paragraphs: &ordinals,
            }])
            .unwrap();
        let preparation = crate::work::WORK.with(|w| w.replace(0));
        preserved_loss::check_candidates(&base, &package, &decoded).unwrap();
        let candidates = crate::work::WORK.with(|w| w.replace(0));
        preserved_loss::check_locations(&decoded, &effects).unwrap();
        let locations = preparation + crate::work::WORK.with(|w| w.get());
        (candidates, locations)
    };
    let small = measure(160, 0);
    let large = measure(320, 0);
    eprintln!("preservation evidence work: 160={small:?}, 320={large:?}");
    for (small, large) in [(small.0, large.0), (small.1, large.1)] {
        assert!(small >= 160 && large > small && large <= small * 2 + 64);
    }
    let small = measure(160, 160);
    let large = measure(320, 320);
    eprintln!("preservation table work: 160={small:?}, 320={large:?}");
    assert_eq!(small.0 * 2, large.0);
    assert!(small.1 >= 160 && large.1 > small.1 && large.1 <= small.1 * 2 + 64);
}
