use super::*;

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
// 이것을 실패시키는 것: 생존 캐시 제거 복원.
#[test]
fn script_tables_and_caches_remain_scoped() {
    let base = scripts(&table_base(), "<opf:itemref idref='aux-h'/>");
    let out = HwpxStructuralEditor::delete_paragraphs(&base, &[TARGET]).unwrap();
    let source = xml(&out);
    assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 2);
    assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 0);
    assert_eq!(source.matches(CACHE).count(), 5);
    let mut package = RawPackage::read(&base).unwrap();
    package.replace_text_entry(
        "Contents/section0.xml",
        xml(&base).replace("treatAsChar=\"0\"", "treatAsChar=\"1\""),
    );
    let bad = package.write().unwrap();
    let out = HwpxStructuralEditor::delete_paragraphs(&bad, &[TARGET]).unwrap();
    assert_eq!(
        xml(&out),
        xml(&bad).replacen(
            &xml(&bad)
                [HwpxDecoder::decode(&bad).unwrap().source_maps[0].paragraphs[1].outer.clone()],
            "",
            1
        )
    );
}

// 이것을 실패시키는 것: preserving 내부 실패 노출 또는 no-op에 admission 적용.
// 이것을 실패시키는 것: delete-layout 체크포인트 제거.
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
        "derived-heading",
        "delete-layout",
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

// Normalize only the declared manifest/spine additions and the normal control comment.
// Scripts payloads stay in both inputs when the order-loss case carries them.
fn without_script_order_items(content: &str) -> String {
    content
        .replace("<!--script editor control-->", "")
        .replace("<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>", "")
        .replace("<opf:item id='aux-h' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
        .replace("<opf:item id='aux-s' href='Scripts/sourceScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
}

fn assert_script_editor_result<T, M, E>(
    expectation: (&str, bool, &mut usize),
    input: &[u8],
    control_input: &[u8],
    actual: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    control: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    view: impl for<'a> Fn(&'a T, &[u8]) -> (&'a [u8], M),
) where
    T: std::fmt::Debug,
    M: std::fmt::Debug + PartialEq,
    E: std::fmt::Debug + std::fmt::Display,
{
    let api = expectation.0;
    assert_editor_result(
        expectation,
        input,
        control_input,
        actual,
        control,
        |path, bytes| {
            if path == "Contents/content.hpf" {
                let original = RawPackage::read(input).unwrap();
                if original.read_text_entry(path).unwrap().contains("idref='aux-h'") {
                    assert_eq!(
                        bytes,
                        original.read_entry(path).unwrap(),
                        "{api}: script manifest preservation"
                    );
                }
                without_script_order_items(std::str::from_utf8(bytes).unwrap()).into_bytes()
            } else {
                bytes.to_vec()
            }
        },
        view,
    )
}

// 이것을 실패시키는 것: script 입력군에서 stamp/patch/fill/insert 성공 결과의 payload 한 바이트 변경 또는 경고 삭제.
// 손실 입력의 기존 오류와 정상 대조 입력의 API별 성공 건수를 함께 고정한다.
#[test]
fn script_other_editors_and_reencode_unchanged() {
    let seed = generated();
    let field = editor_control(&seed, "script editor control");

    let mut successes = [0; 4];
    for (base, has_order, expected) in [
        (seed.clone(), true, [false, true, false, false]),
        (scalar(&seed), true, [false, true, false, false]),
        (inline(&seed, "남음"), true, [false, true, false, false]),
        (body_object(&seed, "남음", &shape()), true, [false, true, false, false]),
        (
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
            true,
            [false, true, false, false],
        ),
        (header_order(&seed), true, [false, true, false, false]),
        (field.clone(), true, [false, true, true, false]),
        // No opaque payload or order loss; the normal pair differs only by an XML comment.
        // This also lets a mutation target just this input rather than its control.
        (field, false, [true, true, true, true]),
    ] {
        let input = if has_order {
            scripts(&base, "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>")
        } else {
            replace(
                &base,
                "Contents/content.hpf",
                "</opf:metadata>",
                "</opf:metadata><!--script editor control-->",
            )
        };
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
        assert_eq!(d.warnings.iter().filter(is_package).count(), usize::from(has_order));
        let control_package = c.warnings.iter().filter(is_package).count();
        assert_eq!(control_package, usize::from(base == header_order(&seed)));
        assert_eq!(
            d.warnings.iter().filter(|w| !is_package(w)).collect::<Vec<_>>(),
            c.warnings.iter().filter(|w| !is_package(w)).collect::<Vec<_>>()
        );
        if control_package == 1 {
            assert_eq!(d.warnings, c.warnings);
        } else {
            assert_eq!(d.warnings.len(), c.warnings.len() + usize::from(has_order));
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
            ("stamp", expected[0], &mut successes[0]),
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
        assert_script_editor_result(
            ("patch", expected[1], &mut successes[1]),
            &input,
            &control,
            patch(&input, &d),
            patch(&control, &c),
            |bytes, _| (bytes, ()),
        );
        let fills = [("field".into(), "채움".into())].into();
        assert_script_editor_result(
            ("fill", expected[2], &mut successes[2]),
            &input,
            &control,
            crate::HwpxFiller::fill_with_codec_diagnostics(&input, &fills),
            crate::HwpxFiller::fill_with_codec_diagnostics(&control, &fills),
            |result, _| (&result.bytes, serde_json::to_value(&result.filled).unwrap()),
        );
        assert_script_editor_result(
            ("insert", expected[3], &mut successes[3]),
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
    assert_eq!(successes, [1, 8, 2, 1], "stamp/patch/fill/insert success counts");
    assert!(successes.into_iter().all(|count| count >= 1));
}

// 이것을 실패시키는 것: 흐름 변경 때 본문 쪽 번호 또는 쪽 참조 허용 복원.
#[test]
fn script_derived_inputs_stay_the_same() {
    let number = |kind| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='DIGIT' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    #[derive(Clone, Copy, Debug)]
    enum Expect {
        Allow,
        RejectDerived,
    }
    let mut cases = vec![
        (number("PAGE"), Expect::RejectDerived),
        (number("TOTAL_PAGE"), Expect::RejectDerived),
        (number("UNKNOWN"), Expect::RejectDerived),
        (number("PAGE").replace("type='DIGIT'", "type='UNKNOWN'"), Expect::RejectDerived),
        (format!("<hp:ctrl><hp:footer><hp:subList><hp:p><hp:run>{}</hp:run></hp:p></hp:subList></hp:footer></hp:ctrl>", number("TOTAL_PAGE")), Expect::RejectDerived),
    ];
    for policy in ["CONTINUOUS", "ON_SECTION", "ON_PAGE", "UNKNOWN"] {
        cases.push((format!("<hp:footNotePr><hp:numbering type='{policy}'/></hp:footNotePr><hp:ctrl><hp:footNote number='1'><hp:subList><hp:p><hp:run><hp:t>각주</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>"), Expect::RejectDerived));
    }
    for (kind, expect) in [
        ("OBJECT_TYPE_PAGE", Expect::RejectDerived),
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
        Expect::Allow,
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
                super::super::delete_layout::check(&p, &xml(input)).map_err(|e| e.to_string()),
            )
        };
        let expected = decision(&base);
        assert_eq!(expected.0.is_ok(), matches!(expect, Expect::Allow), "{content}");
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
                    // Compare semantic content independently of retained layout metadata.
                    expected_document.strip_layout_caches();
                    actual_document.strip_layout_caches();
                    assert_eq!(actual_document, expected_document, "{content}");
                }
                Expect::RejectDerived => {
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
