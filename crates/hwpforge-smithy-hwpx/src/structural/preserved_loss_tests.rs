use super::*;

// Read raw payloads so XML declaration normalization cannot weaken byte checks.
fn replace_raw(base: &[u8], part: &str, from: &str, to: &str) -> Vec<u8> {
    let mut package = RawPackage::read(base).unwrap();
    let source = std::str::from_utf8(package.read_entry(part).unwrap()).unwrap();
    let start = source.find(from).unwrap_or_else(|| panic!("missing {from}"));
    assert_eq!(source.matches(from).count(), 1, "ambiguous replacement {from}");
    let changed = source.replacen(from, to, 1);
    assert_eq!(&changed[..start], &source[..start]);
    assert_eq!(&changed[start + to.len()..], &source[start + from.len()..]);
    package.replace_text_entry(part, changed);
    package.write().unwrap()
}

fn section_no(base: &[u8]) -> Vec<u8> {
    replace_raw(
        base,
        "Contents/content.hpf",
        "idref=\"section0\" linear=\"yes\"",
        "idref=\"section0\" linear=\"no\"",
    )
}

// 이것을 실패시키는 것: 단일 비선형 section의 좁은 허용 비트 비활성화.
#[test]
fn single_section_nonlinear_preserves_source() {
    use crate::decoder::preserved_loss::{LossCause, LossLocation};
    let base = section_no(&carry_base());
    let base = replace_raw(&base, "Contents/content.hpf", "id=\"section0\"", "id=\"body-a\"");
    let base = replace_raw(&base, "Contents/content.hpf", "idref=\"section0\"", "idref=\"body-a\"");
    let mut renamed = RawPackage::read(&base).unwrap();
    let source = renamed.read_entry("Contents/section0.xml").unwrap().to_vec();
    renamed.remove_entries_exact(&["Contents/section0.xml".into()]);
    let renamed = extra(&renamed.write().unwrap(), "Contents/section7.xml", &source);
    let renamed = replace_raw(
        &renamed,
        "Contents/content.hpf",
        "href=\"Contents/section0.xml\"",
        "href=\"Contents/section7.xml\"",
    );
    for (base, part) in [(base, "Contents/section0.xml"), (renamed, "Contents/section7.xml")] {
        original_error(&base);
        let decoded = HwpxDecoder::decode(&base).unwrap();
        let losses = &decoded.source_basis.as_ref().unwrap().losses;
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].cause, LossCause::SingleSectionNonlinearOrder);
        let LossLocation::Package { sections } = &losses[0].location else { panic!() };
        assert_eq!(sections, &[part]);
        let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(
            &base,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        assert_eq!(out.decode_warnings, decoded.warnings);
        assert!(out.encode_warnings.is_empty());
        let before = RawPackage::read(&base).unwrap();
        let after = RawPackage::read(&out.value).unwrap();
        assert_eq!(after.section_path(0).unwrap(), part);
        let source = std::str::from_utf8(before.read_entry(part).unwrap()).unwrap();
        let span = &decoded.source_maps[0].paragraphs[1].outer;
        assert_eq!(
            after.read_entry(part).unwrap(),
            format!("{}{}", &source[..span.start], &source[span.end..]).as_bytes()
        );
        let mut expected = decoded.document;
        expected.sections_mut()[0].paragraphs.remove(1);
        assert_eq!(HwpxDecoder::decode(&out.value).unwrap().document, expected);
        for path in before.entry_paths().filter(|p| *p != part) {
            assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
        }
    }
}

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

// Normalize only the declared manifest/spine additions and the normal control comment.
// Scripts payloads stay in both inputs when the order-loss case carries them.
fn without_script_order_items(content: &str) -> String {
    content
        .replace("<!--script editor control-->", "")
        .replace("<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>", "")
        .replace("<opf:item id='aux-h' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
        .replace("<opf:item id='aux-s' href='Scripts/sourceScripts.js' media-type='application/x-javascript ;charset=utf-16'/>", "")
}

fn assert_editor_result<T, M, E>(
    (api, expected_success, successes): (&str, bool, &mut usize),
    input: &[u8],
    control_input: &[u8],
    actual: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    control: Result<crate::diagnostics::WithCodecWarnings<T>, E>,
    normalize: impl Fn(&str, &[u8]) -> Vec<u8>,
    view: impl for<'a> Fn(&'a T, &[u8]) -> (&'a [u8], M),
) where
    T: std::fmt::Debug,
    M: std::fmt::Debug + PartialEq,
    E: std::fmt::Debug + std::fmt::Display,
{
    assert_eq!(actual.is_ok(), expected_success, "{api}: expected actual success");
    assert_eq!(control.is_ok(), expected_success, "{api}: expected control success");
    match (actual, control) {
        (Ok(actual), Ok(control)) => {
            // The input package warning is intentional; each editor must retain it exactly.
            assert_eq!(
                actual.decode_warnings,
                HwpxDecoder::decode(input).unwrap().warnings,
                "{api}: input decode warnings"
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
                assert_eq!(normalize(path, actual_entry), control_entry, "{api}: {path}");
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
            *successes += 1;
        }
        (Err(actual), Err(control)) => {
            assert_eq!(std::mem::discriminant(&actual), std::mem::discriminant(&control), "{api}");
            assert_eq!(actual.to_string(), control.to_string(), "{api}");
            assert_eq!(format!("{actual:?}"), format!("{control:?}"), "{api}: error fields");
        }
        (actual, control) => panic!("{api}: script={actual:?}, control={control:?}"),
    }
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

fn editor_control(seed: &[u8], label: &str) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(seed).unwrap();
    let mut doc = decoded.document;
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
    doc.sections_mut()[0].paragraphs[0] = paragraph(label);
    crate::HwpxEncoder::encode(&doc.validate().unwrap(), &decoded.style_store, &decoded.image_store)
        .unwrap()
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

// 이것을 실패시키는 것: section 결과 비트에 header 사용 비트를 OR하여 새 원인을 선택.
#[test]
fn nonlinear_header_keeps_its_own_cause() {
    let base = header_order(&carry_base());
    succeeds(&base);
    assert_eq!(
        package_causes(&base),
        [crate::decoder::preserved_loss::LossCause::HeaderOnlyNonlinearOrder]
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
        for base in [header_order(&input), section_no(&input)] {
            succeeds(&base);
            let broken = replace(
                &base,
                "Contents/content.hpf",
                "<opf:title/>",
                "<opf:title><![CDATA[x]]></opf:title>",
            );
            let decoded = HwpxDecoder::decode(&broken).unwrap();
            assert_eq!(decoded.warnings, HwpxDecoder::decode(&base).unwrap().warnings);
            assert!(!decoded
                .source_basis
                .as_ref()
                .unwrap()
                .losses
                .iter()
                .any(|loss| { loss.cause.kind() == "package_reading_order" }));
            assert_eq!(
                HwpxStructuralEditor::delete_paragraphs(&broken, &[TARGET]).unwrap_err(),
                original_error(&broken)
            );
        }
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
    for nonlinear in [false, true] {
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
            let base = if nonlinear { section_no(&base) } else { base };
            let (mut d, p, _) = admit_delete(&base).unwrap();
            let mut basis = d.source_basis.take().unwrap();
            let evidence = &mut std::sync::Arc::make_mut(&mut basis).losses;
            let body_index = evidence
                .iter()
                .position(|loss| matches!(loss.location, LossLocation::Body { .. }))
                .unwrap();
            match change {
                "missing" => evidence.clear(),
                "duplicate" => evidence.push(evidence[body_index].clone()),
                "extra-evidence" => {
                    let mut extra = evidence[body_index].clone();
                    extra.warning = usize::MAX;
                    evidence.push(extra);
                }
                "warning-part" | "relocate-part" => {
                    if let crate::DecodeWarning::ContentLost { part, .. } =
                        &mut d.warnings[evidence[body_index].warning]
                    {
                        *part = "Contents/absent.xml".into();
                    }
                    if change == "relocate-part" {
                        if let LossLocation::Body { part, .. } = &mut evidence[body_index].location
                        {
                            *part = "Contents/absent.xml".into();
                        }
                    }
                }
                "index" => evidence[body_index].warning = usize::MAX,
                "kind" => {
                    if let crate::DecodeWarning::ContentLost { kind, .. } =
                        &mut d.warnings[evidence[body_index].warning]
                    {
                        *kind = "unknown_element".into()
                    }
                }
                "cause" => {
                    evidence[body_index].cause = LossCause::PerLanguageScalar;
                    if let crate::DecodeWarning::ContentLost { kind, .. } =
                        &mut d.warnings[evidence[body_index].warning]
                    {
                        *kind = "char_shape_per_language".into();
                    }
                }
                "part" => {
                    if let LossLocation::Body { part, .. } = &mut evidence[body_index].location {
                        *part = "Contents/missing.xml".into()
                    }
                }
                "digest" => {
                    if let LossLocation::Body { digest, .. } = &mut evidence[body_index].location {
                        digest[0] ^= 1
                    }
                }
                "range" => {
                    if let LossLocation::Body { span, .. } = &mut evidence[body_index].location {
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
                    d.warnings[evidence[body_index].warning] =
                        crate::DecodeWarning::InvalidFieldBoundary {
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
    for (good, section_ref) in [
        (header_order(&carry_base()), "<opf:itemref idref=\"section0\" linear=\"yes\"/>"),
        (
            header_order(&section_no(&carry_base())),
            "<opf:itemref idref=\"section0\" linear=\"no\"/>",
        ),
    ] {
        for (part, from, to) in [
            ("Contents/content.hpf", "linear=\"no\"", "linear=\"maybe\""),
            ("Contents/content.hpf", "idref=\"header\"", "idref=\"header\" future=\"1\""),
            ("Contents/content.hpf", "<opf:spine>", "<opf:spine future=\"1\">"),
            ("Contents/content.hpf", "id=\"header\"", "id=\"header\" future=\"1\""),
            ("Contents/content.hpf", "</opf:spine>", "<opf:itemref idref=\"header\"/></opf:spine>"),
            ("Contents/content.hpf", section_ref, ""),
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
}

// 이것을 실패시키는 것: header 정의 ID·SHA·파트 또는 검증된 package section 순서 검사 생략.
#[test]
fn header_and_package_evidence_cannot_be_relocated() {
    use crate::decoder::preserved_loss::LossLocation;
    for nonlinear in [false, true] {
        for change in
            ["definition", "digest", "part", "sections", "package-part", "location", "absent"]
        {
            let base = if matches!(change, "sections" | "package-part") {
                header_order(&carry_base())
            } else {
                scalar(&carry_base())
            };
            let base = if nonlinear { section_no(&base) } else { base };
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
        for base in [base.clone(), section_no(&base)] {
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

// 이것을 실패시키는 것: check_preserved_loss의 effect.survives(span) 검사 제거.
#[test]
fn source_loss_ranges_are_checked_against_original_effects() {
    use crate::decoder::preserved_loss::LossLocation;
    let base = body_object(&carry_base(), "가 &amp; &lt;나&gt;끝", &shape());
    for base in [base.clone(), section_no(&base)] {
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
        let LossLocation::Body { span, digest, .. } = &d
            .source_basis
            .as_ref()
            .unwrap()
            .losses
            .iter()
            .find(|loss| matches!(loss.location, LossLocation::Body { .. }))
            .unwrap()
            .location
        else {
            panic!()
        };
        assert!(effects.check_preserved_loss(0, span, digest).is_err());
        let untouched = facts.effects(&[]).unwrap();
        untouched.check_preserved_loss(0, span, digest).unwrap();
        assert!(untouched.check_preserved_loss(0, &(span.start..span.end - 1), digest).is_err());
    }
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

fn outline_seed(sections: usize, definitions: usize) -> Vec<u8> {
    use hwpforge_core::{NumberingDef, ParaHead};
    use hwpforge_foundation::NumberFormatType;
    let store = crate::style_store_for_preset("default").unwrap();
    let mut value = serde_json::to_value(&store).unwrap();
    assert_eq!(value["numberings"].as_array().unwrap().len(), store.numbering_count());
    value["numberings"] = serde_json::to_value(
        (0..definitions)
            .map(|i| NumberingDef {
                id: 40 + i as u32,
                start: 1,
                levels: (1..=10)
                    .map(|level| ParaHead {
                        start: 1,
                        level,
                        num_format: NumberFormatType::Digit,
                        text: format!("^{}{}", level, [".", "가)", ")"][i]),
                        checkable: false,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>(),
    )
    .unwrap();
    value["bullets"] = serde_json::json!([{"id":1,"bullet_char":"•","checked_char":null,"use_image":false,"para_head":{"start":1,"level":1,"num_format":serde_json::to_value(NumberFormatType::Digit).unwrap(),"text":"","checkable":false}}]);
    value["styles"][0]["name"] = "Plain".into();
    value["styles"][0]["eng_name"] = "Plain".into();
    let store: crate::HwpxStyleStore = serde_json::from_value(value).unwrap();
    assert_eq!(store.numbering_count(), definitions);
    let mut doc = Document::new();
    for section in 0..sections {
        doc.add_section(Section::with_paragraphs(
            ["start", "delete", "survive"]
                .map(|text| paragraph(&format!("{section} {text}")))
                .to_vec(),
            PageSettings::a4(),
        ));
    }
    let base =
        crate::HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    let package = RawPackage::read(&base).unwrap();
    let header = package.read_text_entry("Contents/header.xml").unwrap();
    assert_eq!(header.matches("<hh:numbering ").count(), definitions);
    if definitions >= 2 {
        assert!(header.contains("^1."));
        assert!(header.contains("^1가)"));
    }
    extra(&base, "Scripts/opaque.bin", b"preserved payload")
}
fn outline_input() -> Vec<u8> {
    replace(
        &outline_seed(2, 2),
        "Contents/section1.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    )
}
fn outline_refused(base: &[u8], targets: &[ParagraphLocator]) {
    let expected = original_error(base);
    let actual = HwpxStructuralEditor::delete_paragraphs(base, targets)
        .expect_err("unexpected accepted deletion");
    assert_eq!(actual, expected);
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert_eq!(actual.to_string(), expected.to_string());
}

// 이것을 실패시키는 것: 유효성 검사 생략 또는 2만 허용.
#[test]
fn outline_other_section_plain_delete() {
    use crate::decoder::preserved_loss::{LossCause, LossLocation};
    use sha2::Digest;
    for id in [2, 3] {
        let base = replace(
            &outline_seed(2, id),
            "Contents/section1.xml",
            "outlineShapeIDRef=\"1\"",
            &format!("outlineShapeIDRef=\"{id}\""),
        );
        let decoded = HwpxDecoder::decode(&base).unwrap();
        let before = RawPackage::read(&base).unwrap();
        let original = before.read_text_entry("Contents/section1.xml").unwrap();
        let start = original.find("<hp:secPr ").unwrap();
        let end = original[start..].find("</hp:secPr>").unwrap() + start + "</hp:secPr>".len();
        let losses = &decoded.source_basis.as_ref().unwrap().losses;
        // 이것을 실패시키는 것: selection 3의 근거 복제/cause 교환/outer 시작 이동/part SHA 대입.
        assert_eq!(losses.len(), 1);
        assert_eq!(losses[0].cause, LossCause::SectionOutlineSelection);
        let LossLocation::SectionProperty { part, span, digest } = &losses[0].location else {
            panic!("expected original section property evidence");
        };
        assert_eq!(part, "Contents/section1.xml");
        assert_eq!(*span, start..end);
        assert_eq!(
            *digest,
            <[u8; 32]>::from(sha2::Sha256::digest(&original.as_bytes()[start..end]))
        );
        let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(
            &base,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        assert_eq!(out.decode_warnings, decoded.warnings);
        assert_eq!(out.decode_warnings.len(), 1);
        let before = RawPackage::read(&base).unwrap();
        let after = RawPackage::read(&out.value).unwrap();
        for part in ["Contents/section1.xml", "Contents/header.xml"] {
            assert_eq!(before.read_entry(part).unwrap(), after.read_entry(part).unwrap());
        }
        let mut expected = decoded.document;
        expected.sections_mut()[0].paragraphs.remove(1);
        assert_eq!(HwpxDecoder::decode(&out.value).unwrap().document, expected);
    }
}

// 이것을 실패시키는 것: section 경로 소유 개수를 1 이상이면 승인.
#[test]
fn outline_duplicate_section_path_refused() {
    let base = outline_input();
    let decoded = HwpxDecoder::decode(&base).unwrap();
    let mut package = RawPackage::read(&base).unwrap();
    preserved_loss::check_candidates(&base, &package, &decoded).unwrap();
    // The real resolver rejects repeated spine targets and decodes using unique
    // file-number fallback. Inject only that private order to reach count == 2.
    package.order.sections[0] = package.order.sections[1].clone();
    assert_eq!(package.section_path(0).unwrap(), package.section_path(1).unwrap());
    let error = preserved_loss::check_candidates(&base, &package, &decoded).unwrap_err();
    assert!(
        matches!(error, crate::HwpxError::InvalidStructure { detail } if detail == "unproven input decode loss")
    );
}

// 이것을 실패시키는 것: 손실 구역 membership 검사 제거.
#[test]
fn outline_same_section_refused() {
    let base = outline_input();
    outline_refused(&base, &[ParagraphLocator { section: 1, index: 1 }]);
    outline_refused(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }, ParagraphLocator { section: 1, index: 1 }],
    );
}

// 이것을 실패시키는 것: found.is_ok() 검사 제거.
#[test]
fn outline_zero_one_and_invalid_id() {
    for id in ["0", "1", "3"] {
        let base = replace(
            &outline_seed(2, 2),
            "Contents/section1.xml",
            "outlineShapeIDRef=\"1\"",
            &format!("outlineShapeIDRef=\"{id}\""),
        );
        let d = HwpxDecoder::decode(&base).unwrap();
        assert_eq!(d.warnings.len(), usize::from(id == "3"));
        assert!(d.source_basis.as_ref().unwrap().losses.is_empty());
        if id == "3" {
            outline_refused(&base, &[ParagraphLocator { section: 0, index: 1 }]);
        }
    }
    let mut missing = RawPackage::read(&outline_seed(2, 2)).unwrap();
    let header = missing.read_text_entry("Contents/header.xml").unwrap();
    let start = header.find("<hh:numberings ").unwrap();
    let end = header.find("</hh:numberings>").unwrap() + 16;
    missing.replace_text_entry(
        "Contents/header.xml",
        format!("{}<hh:numberings itemCnt='0'/>{}", &header[..start], &header[end..]),
    );
    let missing = missing.write().unwrap();
    let decoded = HwpxDecoder::decode(&missing).unwrap();
    assert_eq!(decoded.warnings.iter().filter(|w|matches!(w,crate::DecodeWarning::ContentLost{kind,..}if kind=="secPr.outlineShapeIDRef")).count(), 2);
    assert!(decoded.source_basis.as_ref().unwrap().losses.is_empty());
    outline_refused(&missing, &[ParagraphLocator { section: 0, index: 1 }]);
    for id in ["4294967296", "invalid"] {
        let base = replace(
            &outline_input(),
            "Contents/section1.xml",
            "outlineShapeIDRef=\"2\"",
            &format!("outlineShapeIDRef=\"{id}\""),
        );
        assert!(HwpxDecoder::decode(&base).is_err());
    }
}

fn outline_check(
    base: &[u8],
    targets: &[ParagraphLocator],
    change: impl FnOnce(&mut HwpxDocument),
) -> crate::HwpxResult<()> {
    let mut decoded = HwpxDecoder::decode(base)?;
    change(&mut decoded);
    let package = RawPackage::read(base)?;
    preserved_loss::check_candidates(base, &package, &decoded)?;
    let facts = crate::derived_values::DerivedFacts::collect(&package, &decoded)?;
    let mut sections = std::collections::BTreeMap::<usize, Vec<_>>::new();
    for target in targets {
        sections
            .entry(target.section)
            .or_default()
            .push(facts.body_ordinal(target.section, target.index)?);
    }
    let edits: Vec<_> = sections
        .iter()
        .map(|(&section, paragraphs)| crate::derived_values::DerivedEdit::Delete {
            section,
            paragraphs,
        })
        .collect();
    let effects = facts.effects(&edits)?;
    preserved_loss::check_locations(&decoded, &effects)
}
fn outline_change_heading(
    base: &[u8],
    inherited: bool,
    kind: &str,
    level: u32,
    reference: u32,
) -> Vec<u8> {
    let decoded = HwpxDecoder::decode(base).unwrap();
    let package = RawPackage::read(base).unwrap();
    let shape = decoded.style_store.para_shape(ParaShapeIndex::new(0)).unwrap().clone();
    let index = decoded.style_store.para_shape_count();
    let mut store = decoded.style_store.clone();
    store.push_para_shape(shape);
    let encoded = crate::HwpxEncoder::encode(
        &decoded.document.clone().validate().unwrap(),
        &store,
        &decoded.image_store,
    )
    .unwrap();
    let mut header =
        RawPackage::read(&encoded).unwrap().read_text_entry("Contents/header.xml").unwrap();
    let marker = format!("<hh:paraPr id=\"{index}\"");
    let start = header.find(&marker).unwrap();
    let end = start + header[start..].find("</hh:paraPr>").unwrap() + 12;
    let fragment = &header[start..end];
    let changed = fragment.replace(
        "<hh:heading type=\"NONE\" idRef=\"0\" level=\"0\"",
        &format!("<hh:heading type=\"{kind}\" idRef=\"{reference}\" level=\"{level}\""),
    );
    assert_ne!(fragment, changed);
    header.replace_range(start..end, &changed);
    let mut result = package;
    if inherited {
        let at = header.find("<hh:style ").unwrap();
        let end = at + header[at..].find("/>").unwrap() + 2;
        let style =
            header[at..end].replace("paraPrIDRef=\"0\"", &format!("paraPrIDRef=\"{index}\""));
        header.replace_range(at..end, &style);
    } else {
        let source = result.read_text_entry("Contents/section0.xml").unwrap();
        let span = &decoded.source_maps[0].paragraphs[1].outer;
        let changed =
            source[span.clone()].replace("paraPrIDRef=\"0\"", &format!("paraPrIDRef=\"{index}\""));
        result.replace_text_entry(
            "Contents/section0.xml",
            format!("{}{}{}", &source[..span.start], changed, &source[span.end..]),
        );
    }
    result.replace_text_entry("Contents/header.xml", header);
    result.write().unwrap()
}

// 이것을 실패시키는 것: 개요 근거 없는 scalar 입력에도 heading 제한을 적용하거나 결과/오류를 훼손.
fn outline_legacy_heading_control(base: &[u8]) {
    use crate::decoder::preserved_loss::LossCause;
    let input = scalar(base);
    let decoded = HwpxDecoder::decode(&input).unwrap();
    let losses = &decoded.source_basis.as_ref().unwrap().losses;
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].cause, LossCause::PerLanguageScalar);
    let targets = [ParagraphLocator { section: 0, index: 1 }];
    outline_check(&input, &targets, |_| {}).unwrap();
    // The pre-outline implementation succeeds for all six paired inputs.
    // Compare every output byte and warning, not only the result variant.
    let out =
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&input, &targets).unwrap();
    assert_eq!(out.decode_warnings, decoded.warnings);
    assert!(out.encode_warnings.is_empty());
    let mut expected = RawPackage::read(&input).unwrap();
    let mut section = expected.read_text_entry("Contents/section0.xml").unwrap();
    section.replace_range(decoded.source_maps[0].paragraphs[1].outer.clone(), "");
    expected.replace_text_entry("Contents/section0.xml", section);
    assert_eq!(out.value, expected.write().unwrap());
}

// 이것을 실패시키는 것: 직접 heading/Core heading 검사 제거.
#[test]
fn outline_direct_heading_refused() {
    for kind in ["OUTLINE", "NUMBER", "BULLET"] {
        let input = outline_change_heading(&outline_input(), false, kind, 0, 1);
        // 이것을 실패시키는 것: heading을 가진 삭제 문단만 새 내부 검사에서 건너뛰기.
        assert!(
            outline_check(&input, &[ParagraphLocator { section: 0, index: 1 }], |_| {}).is_err()
        );
        outline_refused(&input, &[ParagraphLocator { section: 0, index: 1 }]);
        let control = replace(
            &input,
            "Contents/section1.xml",
            "outlineShapeIDRef=\"2\"",
            "outlineShapeIDRef=\"1\"",
        );
        outline_legacy_heading_control(&control);
        outline_check(&control, &[ParagraphLocator { section: 0, index: 1 }], |d| {
            d.document.sections_mut()[0].paragraphs[1].heading_level = Some(1)
        })
        .unwrap();
    }
    assert!(outline_check(&outline_input(), &[ParagraphLocator { section: 0, index: 1 }], |d| d
        .document
        .sections_mut()[0]
        .paragraphs[1]
        .heading_level =
        Some(1))
    .is_err());
    let title = replace(
        &outline_input(),
        "Contents/section0.xml",
        "<hp:t>0 delete</hp:t>",
        "<hp:titleMark ignore='false'/><hp:t>0 delete</hp:t>",
    );
    assert_eq!(
        HwpxDecoder::decode(&title).unwrap().document.sections()[0].paragraphs[1].heading_level,
        Some(1)
    );
    // 이것을 실패시키는 것: 원문 titleMark가 있는 삭제 문단만 내부 검사에서 건너뛰기.
    assert!(outline_check(&title, &[ParagraphLocator { section: 0, index: 1 }], |_| {}).is_err());
    outline_refused(&title, &[ParagraphLocator { section: 0, index: 1 }]);
}
// 이것을 실패시키는 것: 스타일 경유 heading 또는 스타일 이름 heading 검사 제거.
#[test]
fn outline_inherited_heading_refused() {
    for kind in ["OUTLINE", "NUMBER", "BULLET"] {
        let input = outline_change_heading(&outline_input(), true, kind, 0, 1);
        // 이것을 실패시키는 것: heading을 가진 삭제 문단만 새 내부 검사에서 건너뛰기.
        assert!(
            outline_check(&input, &[ParagraphLocator { section: 0, index: 1 }], |_| {}).is_err()
        );
        outline_refused(&input, &[ParagraphLocator { section: 0, index: 1 }]);
        let control = replace(
            &input,
            "Contents/section1.xml",
            "outlineShapeIDRef=\"2\"",
            "outlineShapeIDRef=\"1\"",
        );
        outline_legacy_heading_control(&control);
        outline_check(&control, &[ParagraphLocator { section: 0, index: 1 }], |d| {
            d.document.sections_mut()[0].paragraphs[1].heading_level = Some(1)
        })
        .unwrap();
    }
    let input =
        replace(&outline_input(), "Contents/header.xml", "name=\"Plain\"", "name=\"개요 1\"");
    assert!(outline_check(&input, &[ParagraphLocator { section: 0, index: 1 }], |_| {}).is_err());
    outline_refused(&input, &[ParagraphLocator { section: 0, index: 1 }]);
}
// 이것을 실패시키는 것: strict 목록/스타일 조회를 lossy para_list 또는 기본 스타일로 치환.
#[test]
fn outline_unknown_list_and_style_refused() {
    use hwpforge_core::StyleLookup;
    for (kind, level, reference) in
        [("OUTLINE", 10, 0), ("NUMBER", 10, 1), ("BULLET", 10, 1), ("UNKNOWN", 0, 0)]
    {
        let input = outline_change_heading(&outline_input(), false, kind, level, reference);
        let decoded = HwpxDecoder::decode(&input).unwrap();
        let id = decoded.document.sections()[0].paragraphs[1].para_shape_id;
        let shape = decoded.style_store.para_shape(id).unwrap();
        assert!(decoded.style_store.para_list(id).is_none());
        // Unknown headings retain their independent decode loss; known invalid lists return Err.
        assert_eq!(decoded.style_store.para_list_ref(shape).is_err(), kind != "UNKNOWN");
        outline_refused(&input, &[ParagraphLocator { section: 0, index: 1 }]);
    }
    let base = outline_input();
    let char_style = replace(
        &base,
        "Contents/header.xml",
        "type=\"PARA\" name=\"Plain\"",
        "type=\"CHAR\" name=\"Plain\"",
    );
    outline_refused(&char_style, &[ParagraphLocator { section: 0, index: 1 }]);
    for change in [0, 1, 2] {
        assert!(outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
            let p = &mut d.document.sections_mut()[0].paragraphs[1];
            match change {
                0 => p.style_ref = None,
                1 => p.para_shape_id = ParaShapeIndex::new(99),
                _ => p.style_ref = Some(hwpforge_foundation::StyleRef::new(99)),
            }
        })
        .is_err());
    }
    for (from, to) in
        [("paraPrIDRef=\"0\"", "paraPrIDRef=\"999\""), ("styleIDRef=\"0\"", "styleIDRef=\"999\"")]
    {
        let input = replace(&base, "Contents/section0.xml", from, to);
        assert!(HwpxDecoder::decode(&input).is_err());
    }
}

// 이것을 실패시키는 것: StyleRef::get()을 wire 위치로 사용.
#[test]
fn outline_style_reference_is_not_position() {
    let base = outline_input();
    outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
        let mut value = serde_json::to_value(&d.style_store).unwrap();
        let positions = value["style_positions"].as_array_mut().unwrap();
        positions.swap(0, 1);
        d.style_store = serde_json::from_value(value).unwrap();
        d.document.sections_mut()[0].paragraphs[1].style_ref = d.style_store.style_ref_at(0);
        assert_ne!(d.document.sections()[0].paragraphs[1].style_ref.unwrap().get(), 0);
    })
    .unwrap();
    assert!(outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
        let mut value = serde_json::to_value(&d.style_store).unwrap();
        value["style_positions"].as_array_mut().unwrap().swap(0, 1);
        value["styles"][0]["name"] = "개요 1".into();
        d.style_store = serde_json::from_value(value).unwrap();
        d.document.sections_mut()[0].paragraphs[1].style_ref = d.style_store.style_ref_at(0);
    })
    .is_err());
}

// 이것을 실패시키는 것: 첫 문단만 검사 또는 any(NONE)으로 배치 승인.
#[test]
fn outline_all_deleted_paragraphs_checked() {
    let base = replace(
        &outline_seed(3, 2),
        "Contents/section2.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    );
    let targets = [
        ParagraphLocator { section: 0, index: 1 },
        ParagraphLocator { section: 0, index: 2 },
        ParagraphLocator { section: 1, index: 1 },
    ];
    HwpxStructuralEditor::delete_paragraphs(&base, &targets).unwrap();
    for target in targets {
        let changed = outline_change_heading(&base, false, "NUMBER", 0, 1);
        let heading = HwpxDecoder::decode(&changed).unwrap().document.sections()[0].paragraphs[1]
            .para_shape_id
            .get();
        let mut package = RawPackage::read(&changed).unwrap();
        package.replace_text_entry(
            "Contents/section0.xml",
            RawPackage::read(&base).unwrap().read_text_entry("Contents/section0.xml").unwrap(),
        );
        let part = format!("Contents/section{}.xml", target.section);
        let decoded = HwpxDecoder::decode(&base).unwrap();
        let source = package.read_text_entry(&part).unwrap();
        let span = &decoded.source_maps[target.section].paragraphs[target.index].outer;
        let paragraph = source[span.clone()]
            .replace("paraPrIDRef=\"0\"", &format!("paraPrIDRef=\"{heading}\""));
        package.replace_text_entry(
            &part,
            format!("{}{}{}", &source[..span.start], paragraph, &source[span.end..]),
        );
        outline_refused(&package.write().unwrap(), &targets);
        assert!(outline_check(&base, &targets, |d| d.document.sections_mut()[target.section]
            .paragraphs[target.index]
            .heading_level = Some(1))
        .is_err());
    }
}

// 이것을 실패시키는 것: 동일 secPr 중복 허용 또는 source map section 소유 덮어쓰기.
#[test]
fn outline_each_section_needs_its_own_evidence() {
    use crate::decoder::preserved_loss::{LossCause, LossLocation};
    let base = replace(
        &outline_seed(3, 2),
        "Contents/section1.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    );
    let base = replace(
        &base,
        "Contents/section2.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    );
    let d = HwpxDecoder::decode(&base).unwrap();
    let losses = &d.source_basis.as_ref().unwrap().losses;
    assert_eq!(losses.len(), 2);
    for (i, loss) in losses.iter().enumerate() {
        assert_eq!(loss.warning, i);
        assert_eq!(loss.cause, LossCause::SectionOutlineSelection);
        let LossLocation::SectionProperty { part, span, digest } = &loss.location else { panic!() };
        assert_eq!(part, &format!("Contents/section{}.xml", i + 1));
        let xml = RawPackage::read(&base).unwrap().read_text_entry(part).unwrap();
        use sha2::Digest;
        assert_eq!(span.start, xml.find("<hp:secPr ").unwrap());
        assert_eq!(span.end, xml.find("</hp:secPr>").unwrap() + 11);
        assert_eq!(*digest, <[u8; 32]>::from(sha2::Sha256::digest(&xml.as_bytes()[span.clone()])));
    }
    HwpxStructuralEditor::delete_paragraphs(&base, &[ParagraphLocator { section: 0, index: 1 }])
        .unwrap();
    for change in ["missing", "copied", "same-property", "owner"] {
        assert!(
            outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
                let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
                match change {
                    "missing" => {
                        basis.losses.pop();
                    }
                    "copied" => basis.losses[1].location = basis.losses[0].location.clone(),
                    "same-property" => {
                        basis.losses[1].location = basis.losses[0].location.clone();
                        let crate::DecodeWarning::ContentLost { part, .. } = &mut d.warnings[1]
                        else {
                            panic!()
                        };
                        *part = "Contents/section1.xml".into();
                    }
                    "owner" => d.source_maps[2] = d.source_maps[1].clone(),
                    _ => unreachable!(),
                }
            })
            .is_err(),
            "{change}"
        );
    }
    let mut duplicate = HwpxDecoder::decode(&base).unwrap();
    let basis = std::sync::Arc::make_mut(duplicate.source_basis.as_mut().unwrap());
    basis.losses[1].location = basis.losses[0].location.clone();
    let crate::DecodeWarning::ContentLost { part, .. } = &mut duplicate.warnings[1] else {
        panic!()
    };
    *part = "Contents/section1.xml".into();
    assert!(preserved_loss::check_candidates(&base, &RawPackage::read(&base).unwrap(), &duplicate)
        .is_err());
}

// 이것을 실패시키는 것: package/span digest·cause/location 짝·index·kind·part 대조 각각 생략.
#[test]
fn outline_private_evidence_is_one_to_one() {
    use crate::decoder::preserved_loss::{LossCause, LossLocation};
    let base = scalar(&outline_input());
    for change in [
        "missing",
        "extra",
        "index",
        "order",
        "kind",
        "part",
        "span-start",
        "span-end",
        "digest",
        "cause",
        "cause-with-kind",
        "span-cropped-digest",
        "body",
        "run",
        "package-part",
    ] {
        assert!(
            outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
                let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
                assert_eq!(basis.losses.len(), 2);
                match change {
                    "missing" => {
                        basis.losses.pop();
                    }
                    "extra" => basis.losses.push(basis.losses[1].clone()),
                    "index" => basis.losses[1].warning = 99,
                    "order" => basis.losses.swap(0, 1),
                    "kind" => {
                        let crate::DecodeWarning::ContentLost { kind, .. } = &mut d.warnings[1]
                        else {
                            panic!()
                        };
                        *kind = "unknown".into();
                    }
                    "part" => {
                        let crate::DecodeWarning::ContentLost { part, .. } = &mut d.warnings[1]
                        else {
                            panic!()
                        };
                        *part = "Contents/section0.xml".into();
                    }
                    "cause" => basis.losses[1].cause = LossCause::InlineStyle,
                    "cause-with-kind" => {
                        basis.losses[1].cause = LossCause::InlineStyle;
                        let crate::DecodeWarning::ContentLost { kind, .. } = &mut d.warnings[1]
                        else {
                            panic!()
                        };
                        *kind = "t.charStyleIDRef".into();
                    }
                    "span-cropped-digest" => {
                        let LossLocation::SectionProperty { part, span, digest } =
                            &mut basis.losses[1].location
                        else {
                            panic!()
                        };
                        span.end -= 1;
                        let source =
                            RawPackage::read(&base).unwrap().read_text_entry(part).unwrap();
                        use sha2::Digest;
                        *digest = sha2::Sha256::digest(&source.as_bytes()[span.clone()]).into();
                    }
                    "body" => {
                        let LossLocation::SectionProperty { part, span, digest } =
                            basis.losses[1].location.clone()
                        else {
                            panic!()
                        };
                        basis.losses[1].location = LossLocation::Body { part, span, digest };
                    }
                    "run" => {
                        let LossLocation::SectionProperty { span, .. } =
                            &mut basis.losses[1].location
                        else {
                            panic!()
                        };
                        *span = d.source_maps[1]
                            .nodes()
                            .find(|n| n.kind() == hwpforge_core::node_path::NodeKind::Run)
                            .unwrap()
                            .ranges()[0]
                            .clone();
                    }
                    _ => {
                        let LossLocation::SectionProperty { part, span, digest } =
                            &mut basis.losses[1].location
                        else {
                            panic!()
                        };
                        match change {
                            "span-start" => span.start += 1,
                            "span-end" => span.end -= 1,
                            "digest" => digest[0] ^= 1,
                            "package-part" => *part = "Contents/header.xml".into(),
                            _ => unreachable!(),
                        };
                    }
                }
            })
            .is_err(),
            "{change}"
        );
    }
    let (mut extra, package, _) = admit_delete(&base).unwrap();
    let basis = std::sync::Arc::make_mut(extra.source_basis.as_mut().unwrap());
    basis.losses.push(basis.losses[1].clone());
    assert!(preserved_loss::check_candidates(&base, &package, &extra).is_err());
    let (mut d, p, _) = admit_delete(&base).unwrap();
    assert!(preserved_loss::check_candidates(&outline_input(), &p, &d).is_err());
    let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
    let LossLocation::SectionProperty { part, .. } = &mut basis.losses[1].location else {
        panic!()
    };
    *part = "Contents/header.xml".into();
    let crate::DecodeWarning::ContentLost { part, .. } = &mut d.warnings[1] else { panic!() };
    *part = "Contents/header.xml".into();
    assert!(preserved_loss::check_candidates(&base, &p, &d).is_err());
    let mixed = inline(&scalar(&outline_input()), "0 survive");
    outline_check(&mixed, &[ParagraphLocator { section: 0, index: 1 }], |d| {
        assert_eq!(
            d.source_basis.as_ref().unwrap().losses.iter().map(|l| l.cause).collect::<Vec<_>>(),
            [
                LossCause::PerLanguageScalar,
                LossCause::InlineStyle,
                LossCause::SectionOutlineSelection
            ]
        );
    })
    .unwrap();
    for index in [1, 2] {
        assert!(outline_check(&mixed, &[ParagraphLocator { section: 0, index: 1 }], |d| {
            let loss =
                &mut std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap()).losses[index];
            match &mut loss.location {
                LossLocation::Body { digest, .. }
                | LossLocation::SectionProperty { digest, .. } => digest[0] ^= 1,
                _ => panic!(),
            }
        })
        .is_err());
    }
}

// 이것을 실패시키는 것: reason의 validated numbering 문구로 면제.
#[test]
fn outline_warning_text_is_not_policy() {
    let base = outline_input();
    outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
        let crate::DecodeWarning::ContentLost { reason, .. } = &mut d.warnings[0] else { panic!() };
        *reason = "opaque unrelated sentence".into();
    })
    .unwrap();
    assert!(outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
        std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap()).losses.clear();
        let crate::DecodeWarning::ContentLost { reason, .. } = &mut d.warnings[0] else { panic!() };
        *reason = "validated numbering 2".into();
    })
    .is_err());
}
// 이것을 실패시키는 것: package 근거가 section 근거를 대체 또는 전역 warning offset 중복.
#[test]
fn outline_and_script_evidence_coexist() {
    use crate::decoder::preserved_loss::LossCause;
    let base =
        scripts(&outline_input(), "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>");
    let d = HwpxDecoder::decode(&base).unwrap();
    assert_eq!(
        d.source_basis
            .as_ref()
            .unwrap()
            .losses
            .iter()
            .map(|l| (l.warning, l.cause))
            .collect::<Vec<_>>(),
        [(0, LossCause::ScriptAuxiliaryOrder), (1, LossCause::SectionOutlineSelection)]
    );
    HwpxStructuralEditor::delete_paragraphs(&base, &[ParagraphLocator { section: 0, index: 1 }])
        .unwrap();
    for change in [0, 1, 2] {
        assert!(outline_check(&base, &[ParagraphLocator { section: 0, index: 1 }], |d| {
            let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
            match change {
                0 | 1 => {
                    basis.losses.remove(change);
                }
                _ => basis.losses.swap(0, 1),
            }
        })
        .is_err());
    }
}

// 이것을 실패시키는 것: 최종 보존/Preview 검증 또는 원래 admission 오류 복원 생략.
#[test]
fn outline_final_payload_corruption_refused() {
    let base = scalar(&scripts(
        &outline_input(),
        "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>",
    ));
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
        ("Contents/section1.xml", "outlineShapeIDRef=\"2\"", "outlineShapeIDRef=\"0\""),
        ("Contents/section1.xml", "<hp:grid ", "<hp:grid changed='1' "),
        ("Contents/section1.xml", "survive", "survivx"),
        ("Contents/header.xml", "^1가)", "^1나)"),
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

// 이것을 실패시키는 것: 최종 보존/Preview 검증 또는 원래 admission 오류 복원 생략.
#[test]
fn outline_preview_removal_is_bounded() {
    let base = extra(
        &extra(
            &scripts(&outline_input(), "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>"),
            "Preview/PrvText.txt",
            b"preview",
        ),
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

// 이것을 실패시키는 것: 최종 보존/Preview 검증 또는 원래 admission 오류 복원 생략.
#[test]
fn outline_original_admission_error_and_noop() {
    let base =
        scripts(&outline_input(), "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>");
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

// 이것을 실패시키는 것: 개요 근거가 다른 ContentLost 또는 비선형 body 손실까지 면제.
#[test]
fn outline_other_losses_and_nonlinear_remain_refused() {
    let base = outline_input();
    for content in [
        "<hp:ctrl><hp:autoNum num='1' numType='UNKNOWN'/></hp:ctrl>",
        "<hp:ctrl><hp:fieldBegin id='10' type='CROSSREF' dirty='0'><hp:parameters><hp:stringParam name='RefType'>TARGET_OUTLINE</hp:stringParam><hp:stringParam name='RefContentType'>OBJECT_TYPE_NUMBER</hp:stringParam><hp:stringParam name='RefPath'>?mark;</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl><hp:t>3</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>",
        "<hp:ole id='1'/>",
    ] {
        let input=replace(&base,"Contents/section0.xml","<hp:t>0 survive</hp:t>",content);
        outline_refused(&input,&[ParagraphLocator {section:0,index:1}]);
    }
    let input = replace(
        &base,
        "Contents/header.xml",
        "<hh:heading type=\"NONE\"",
        "<hh:heading type=\"UNKNOWN\"",
    );
    outline_refused(&input, &[ParagraphLocator { section: 0, index: 1 }]);
    for nonlinearity in [0, 1] {
        let input = replace(
            &base,
            "Contents/content.hpf",
            &format!("idref=\"section{nonlinearity}\" linear=\"yes\""),
            &format!("idref=\"section{nonlinearity}\" linear=\"no\""),
        );
        for target in [0, 1] {
            outline_refused(&input, &[ParagraphLocator { section: target, index: 1 }]);
            let combined =
                scripts(&input, "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>");
            outline_refused(&combined, &[ParagraphLocator { section: target, index: 1 }]);
        }
    }
}

// 이것을 실패시키는 것: recorder 없는 auxiliary 호출에 section 위치를 추정하여 생성.
#[test]
fn outline_masterpage_and_header_have_no_evidence() {
    use hwpforge_core::section::MasterPage;
    use hwpforge_foundation::ApplyPageType;
    let d = HwpxDecoder::decode(&outline_seed(2, 2)).unwrap();
    let mut doc = d.document;
    doc.sections_mut()[0].master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![paragraph("master")])]);
    let base = crate::HwpxEncoder::encode(&doc.validate().unwrap(), &d.style_store, &d.image_store)
        .unwrap();
    let mut package = RawPackage::read(&base).unwrap();
    let properties = package.read_text_entry("Contents/section1.xml").unwrap();
    let a = properties.find("<hp:secPr ").unwrap();
    let b = properties.find("</hp:secPr>").unwrap() + 11;
    let properties = properties[a..b].replace("outlineShapeIDRef=\"1\"", "outlineShapeIDRef=\"2\"");
    let master = package
        .entry_paths()
        .find(|p| p.contains("masterpage") && p.ends_with(".xml"))
        .unwrap()
        .to_string();
    let xml = package
        .read_text_entry(&master)
        .unwrap()
        .replace("<hp:t>master</hp:t>", &format!("{properties}<hp:t>master</hp:t>"));
    package.replace_text_entry(&master, xml);
    let base = extra(&package.write().unwrap(), "Scripts/opaque.bin", b"payload");
    let decoded = HwpxDecoder::decode(&base).unwrap();
    assert!(decoded.warnings.iter().any(|w|matches!(w,crate::DecodeWarning::ContentLost{kind,..} if kind=="secPr.outlineShapeIDRef")));
    assert!(decoded.source_basis.as_ref().unwrap().losses.is_empty());
    outline_refused(&base, &[ParagraphLocator { section: 0, index: 1 }]);
    let refs = crate::header_references::HeaderReferences::build(&decoded.style_store).unwrap();
    for part in ["Contents/header.xml", "Contents/masterpage0.xml"] {
        assert_eq!(refs.validate_auxiliary_xml("<hp:secPr xmlns:hp='http://www.hancom.co.kr/hwpml/2011/paragraph' outlineShapeIDRef='2'/>",part).unwrap().len(),1);
    }
}

// 이것을 실패시키는 것: 번호/표 높이/줄 cache gate 우회.
#[test]
fn outline_tables_and_caches_stay_scoped() {
    let base = outline_input();
    let auto = |kind, format| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='{format}' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    for kind in ["PAGE", "TOTAL_PAGE"] {
        let good =
            replace(&base, "Contents/section0.xml", "<hp:t>0 survive</hp:t>", &auto(kind, "DIGIT"));
        HwpxStructuralEditor::delete_paragraphs(
            &good,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap();
        let bad = replace(
            &base,
            "Contents/section0.xml",
            "<hp:t>0 survive</hp:t>",
            &auto(kind, "UNKNOWN"),
        );
        outline_refused(&bad, &[ParagraphLocator { section: 0, index: 1 }]);
    }
    for policy in ["CONTINUOUS", "ON_SECTION", "ON_PAGE", "UNKNOWN"] {
        let note=format!("<hp:footNotePr><hp:numbering type='{policy}'/></hp:footNotePr><hp:ctrl><hp:footNote number='1'><hp:subList><hp:p><hp:run><hp:t>note</hp:t></hp:run></hp:p></hp:subList></hp:footNote></hp:ctrl>");
        let bad = replace(&base, "Contents/section0.xml", "<hp:t>0 survive</hp:t>", &note);
        outline_refused(&bad, &[ParagraphLocator { section: 0, index: 1 }]);
    }
    let mut pkg = RawPackage::read(&base).unwrap();
    pkg.replace_text_entry("Contents/section0.xml", xml(&table_base()));
    let table = pkg.write().unwrap();
    let output = HwpxStructuralEditor::delete_paragraphs(
        &table,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    let source = xml(&output);
    assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 1);
    assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 1);
    assert_eq!(source.matches("<hp:linesegarray>").count(), 3);
    let mut bad = RawPackage::read(&table).unwrap();
    bad.replace_text_entry(
        "Contents/section0.xml",
        xml(&table).replace("treatAsChar=\"0\"", "treatAsChar=\"1\""),
    );
    let bad = bad.write().unwrap();
    outline_refused(&bad, &[ParagraphLocator { section: 0, index: 1 }]);
}

// 이것을 실패시키는 것: outline 입력군에서 stamp/patch/fill/insert 성공 결과의 payload 한 바이트 변경 또는 경고 삭제.
// 손실 입력의 오류 비교와 일반 re-encode의 선택 정규화도 유지한다.
#[test]
fn outline_other_editors_and_reencode_unchanged() {
    let mut successes = [0; 4];
    for (base, has_loss, expected) in [
        (outline_input(), true, [false, true, false, false]),
        (scalar(&outline_input()), true, [false, true, false, false]),
        (
            scripts(&outline_input(), "<opf:itemref idref='aux-h'/><opf:itemref idref='aux-s'/>"),
            true,
            [false, true, false, false],
        ),
        // No loss or opaque payload; the input/control differ only by an XML comment.
        (
            replace(
                &editor_control(&outline_input(), "outline editor control"),
                "Contents/content.hpf",
                "</opf:metadata>",
                "</opf:metadata><!--outline editor control-->",
            ),
            false,
            [true, true, true, true],
        ),
    ] {
        let control = if has_loss {
            replace(
                &base,
                "Contents/section1.xml",
                "outlineShapeIDRef=\"2\"",
                "outlineShapeIDRef=\"1\"",
            )
        } else {
            replace(&base, "Contents/content.hpf", "<!--outline editor control-->", "")
        };
        let d = HwpxDecoder::decode(&base).unwrap();
        let c = HwpxDecoder::decode(&control).unwrap();
        assert_eq!(d.document, c.document);
        assert_eq!(d.style_store, c.style_store);
        assert_eq!(d.image_store, c.image_store);
        let reencode = |d: &HwpxDocument| {
            crate::HwpxEncoder::encode_with_diagnostics(
                &d.document.clone().validate().unwrap(),
                &d.style_store,
                &d.image_store,
                crate::EncodeOptions::default(),
            )
            .unwrap()
        };
        let a = reencode(&d);
        let b = reencode(&c);
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.warnings, b.warnings);
        assert!(RawPackage::read(&a.bytes)
            .unwrap()
            .read_text_entry("Contents/section1.xml")
            .unwrap()
            .contains("outlineShapeIDRef=\"1\""));
        let normalize = |path: &str, bytes: &[u8]| {
            if has_loss && path == "Contents/section1.xml" {
                assert_eq!(bytes, RawPackage::read(&base).unwrap().read_entry(path).unwrap());
                std::str::from_utf8(bytes)
                    .unwrap()
                    .replace("outlineShapeIDRef=\"2\"", "outlineShapeIDRef=\"1\"")
                    .into_bytes()
            } else if path == "Contents/content.hpf" {
                std::str::from_utf8(bytes)
                    .unwrap()
                    .replace("<!--outline editor control-->", "")
                    .into_bytes()
            } else {
                bytes.to_vec()
            }
        };
        assert_editor_result(
            ("stamp", expected[0], &mut successes[0]),
            &base,
            &control,
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&base, &[]),
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&control, &[]),
            normalize,
            |r, input| {
                assert_eq!(r.manifest.source_sha256, crate::patch::sha256_hex(input));
                assert_eq!(r.manifest.output_sha256, crate::patch::sha256_hex(&r.bytes));
                (
                    &r.bytes,
                    (
                        r.manifest.schema_version,
                        serde_json::to_value(&r.manifest.fields).unwrap(),
                        r.outcome.clone(),
                    ),
                )
            },
        );
        let patch = |input: &[u8], d: &HwpxDocument| {
            let mut section = d.document.sections()[0].clone();
            section.paragraphs[1] = paragraph("edited\u{1}");
            let p = crate::HwpxPatcher::export_section_preservation(
                input,
                0,
                &d.document.sections()[0],
            )
            .unwrap();
            crate::HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
                input,
                0,
                &section,
                None,
                Some(&p),
            )
        };
        assert_editor_result(
            ("patch", expected[1], &mut successes[1]),
            &base,
            &control,
            patch(&base, &d),
            patch(&control, &c),
            normalize,
            |bytes, _| (bytes, ()),
        );
        let fills = [("field".into(), "value".into())].into();
        assert_editor_result(
            ("fill", expected[2], &mut successes[2]),
            &base,
            &control,
            crate::HwpxFiller::fill_with_codec_diagnostics(&base, &fills),
            crate::HwpxFiller::fill_with_codec_diagnostics(&control, &fills),
            normalize,
            |r, _| (&r.bytes, serde_json::to_value(&r.filled).unwrap()),
        );
        assert_editor_result(
            ("insert", expected[3], &mut successes[3]),
            &base,
            &control,
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &base,
                ParagraphLocator { section: 0, index: 1 },
                InsertPosition::After,
                &["insert".into()],
            ),
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &control,
                ParagraphLocator { section: 0, index: 1 },
                InsertPosition::After,
                &["insert".into()],
            ),
            normalize,
            |bytes, _| (bytes, ()),
        );
    }
    assert_eq!(successes, [1, 4, 1, 1], "stamp/patch/fill/insert success counts");
    assert!(successes.into_iter().all(|count| count >= 1));
}

// 이것을 실패시키는 것: 개요 손실 근거마다 전체 source map을 다시 순회.
// 삭제 문단/InlineStyle 손실 축과 개요 손실 구역 축을 각각 두 배로 늘린다.
#[test]
fn outline_work_is_bounded() {
    let measure = |n: usize, outline_sections: usize| {
        let base = outline_seed(outline_sections + 1, 2);
        let mut package = RawPackage::read(&base).unwrap();
        let source = package.read_text_entry("Contents/section0.xml").unwrap();
        let d = HwpxDecoder::decode(&base).unwrap();
        let fragment = &source[d.source_maps[0].paragraphs[2].outer.clone()];
        package.replace_text_entry(
            "Contents/section0.xml",
            source.replace(
                "</hs:sec>",
                &format!(
                    "{}{}</hs:sec>",
                    fragment.repeat(n),
                    fragment.replace("<hp:t>", "<hp:t charStyleIDRef='0'>").repeat(n)
                ),
            ),
        );
        for section in 1..=outline_sections {
            let part = format!("Contents/section{section}.xml");
            let source = package
                .read_text_entry(&part)
                .unwrap()
                .replace("outlineShapeIDRef=\"1\"", "outlineShapeIDRef=\"2\"");
            package.replace_text_entry(&part, source);
        }
        let base = package.write().unwrap();
        let before = HwpxDecoder::decode(&base).unwrap();
        assert_eq!(
            before
                .source_basis
                .as_ref()
                .unwrap()
                .losses
                .iter()
                .filter(|loss| {
                    loss.cause == crate::decoder::preserved_loss::LossCause::SectionOutlineSelection
                })
                .count(),
            outline_sections
        );
        let targets: Vec<_> = (1..=n).map(|index| ParagraphLocator { section: 0, index }).collect();
        crate::derived_values::COLLECTIONS.with(|c| c.set(0));
        crate::work::WORK.with(|w| w.set(0));
        crate::derived_values::OUTLINE_BATCHES.with(|c| c.set(0));
        let output = HwpxStructuralEditor::delete_paragraphs(&base, &targets).unwrap();
        assert_eq!(crate::derived_values::OUTLINE_BATCHES.with(std::cell::Cell::get), 1);
        let work = crate::work::WORK.with(std::cell::Cell::get);
        assert_eq!(crate::derived_values::COLLECTIONS.with(std::cell::Cell::get), 1);
        let decoded = HwpxDecoder::decode(&output).unwrap();
        assert_eq!(decoded.document.sections()[0].paragraphs.len(), n + 3);
        assert_eq!(
            decoded.warnings.iter().filter(|w| w.is_content_loss()).count(),
            n + outline_sections
        );
        work
    };
    for (small, large) in
        [(measure(160, 2), measure(320, 2)), (measure(160, 160), measure(320, 320))]
    {
        assert!(large > small);
        assert!(large <= small * 2 + 256, "{small} {large}");
    }
}

// 이것을 실패시키는 것: 비교 helper가 호출자의 기대 실패를 기대 성공으로 덮어씀.
// payload/metadata/decode/encode 경고 훼손도 계속 거부해야 한다.
#[test]
fn outline_editor_comparison_detects_corruption() {
    use crate::diagnostics::WithCodecWarnings;
    let base = outline_input();
    let control = replace(
        &base,
        "Contents/section1.xml",
        "outlineShapeIDRef=\"2\"",
        "outlineShapeIDRef=\"1\"",
    );
    let diagnosed = |input: &[u8]| {
        WithCodecWarnings::new(
            (input.to_vec(), 0u8),
            HwpxDecoder::decode(input).unwrap().warnings,
            vec![],
        )
    };
    for change in ["payload", "metadata", "decode", "control-decode", "encode", "outcome"] {
        let mut actual = diagnosed(&base);
        let mut expected = diagnosed(&control);
        match change {
            "payload" => {
                actual.value.0 =
                    replace(&actual.value.0, "Contents/section0.xml", "0 delete", "changed")
            }
            "metadata" => actual.value.1 = 1,
            "decode" => actual.decode_warnings.clear(),
            "control-decode" => expected.decode_warnings.push(crate::DecodeWarning::ContentLost {
                part: "Contents/section0.xml".into(),
                path: Default::default(),
                kind: "unknown".into(),
                reason: "lost".into(),
            }),
            "encode" => actual.encode_warnings.push(crate::EncodeWarning::PreviewRemoved {
                parts: vec!["Preview/PrvText.txt".into()],
            }),
            "outcome" => {} // Paired Ok values must not satisfy an expected failure.
            _ => unreachable!(),
        }
        let result = std::panic::catch_unwind(|| {
            let mut successes = 0;
            assert_editor_result::<_, _, crate::HwpxError>(
                ("oracle", change != "outcome", &mut successes),
                &base,
                &control,
                Ok(actual),
                Ok(expected),
                |path, bytes| match path {
                    "Contents/section1.xml" => std::str::from_utf8(bytes)
                        .unwrap()
                        .replace("outlineShapeIDRef=\"2\"", "outlineShapeIDRef=\"1\"")
                        .into_bytes(),
                    _ => bytes.to_vec(),
                },
                |r, _| (&r.0, r.1),
            )
        });
        assert!(result.is_err(), "{change}");
    }
}

// 이것을 실패시키는 것: part 소유 section 개수를 무시하고 하나를 선택하거나 근거 소비에서 XML 전체 재파싱.
#[test]
fn outline_part_owners_are_unique() {
    let base = replace(
        &outline_seed(3, 2),
        "Contents/section1.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    );
    let base = replace(
        &base,
        "Contents/section2.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef=\"2\"",
    );
    let original = HwpxDecoder::decode(&base).unwrap();
    let package = RawPackage::read(&base).unwrap();
    let facts = crate::derived_values::DerivedFacts::collect(&package, &original).unwrap();
    let ordinals = [facts.body_ordinal(0, 1).unwrap()];
    let effects = facts
        .effects(&[crate::derived_values::DerivedEdit::Delete {
            section: 0,
            paragraphs: &ordinals,
        }])
        .unwrap();
    let before = hwpforge_core::source_xml::source_work();
    preserved_loss::check_locations(&original, &effects).unwrap();
    let after = hwpforge_core::source_xml::source_work();
    assert_eq!((after.0, after.1), (before.0, before.1));
    let mut ambiguous = HwpxDecoder::decode(&base).unwrap();
    ambiguous.source_maps[2] = ambiguous.source_maps[1].clone();
    std::sync::Arc::make_mut(ambiguous.source_basis.as_mut().unwrap()).losses.truncate(1);
    assert!(preserved_loss::check_locations(&ambiguous, &effects).is_err());
}

fn package_causes(base: &[u8]) -> Vec<crate::decoder::preserved_loss::LossCause> {
    HwpxDecoder::decode(base)
        .unwrap()
        .source_basis
        .as_ref()
        .unwrap()
        .losses
        .iter()
        .filter(|loss| loss.cause.kind() == "package_reading_order")
        .map(|loss| loss.cause)
        .collect()
}

fn no_single_evidence(base: &[u8], section: usize) {
    assert!(package_causes(base).is_empty());
    let expected = original_error(base);
    let actual =
        HwpxStructuralEditor::delete_paragraphs(base, &[ParagraphLocator { section, index: 1 }])
            .unwrap_err();
    assert_eq!(actual, expected);
    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
    assert_eq!(actual.to_string(), expected.to_string());
}

// 이것을 실패시키는 것: 방문 단계에서 정확한 no 값 검사 제거.
#[test]
fn single_section_linear_values_are_exact() {
    use crate::decoder::preserved_loss::LossCause::SingleSectionNonlinearOrder;
    for (attribute, nonlinear, allowed) in [
        ("", false, true),
        (" linear='yes'", false, true),
        (" linear='no'", true, true),
        (" linear='n&#111;'", true, true),
        (" linear=''", false, false),
        (" linear='NO'", false, false),
        (" linear='false'", false, false),
        (" linear=' no'", false, false),
        (" linear='no '", false, false),
    ] {
        let base = replace_raw(
            &carry_base(),
            "Contents/content.hpf",
            "idref=\"section0\" linear=\"yes\"",
            &format!("idref='section0'{attribute}"),
        );
        assert_eq!(
            package_causes(&base),
            if nonlinear { vec![SingleSectionNonlinearOrder] } else { vec![] },
            "{attribute}"
        );
        if allowed {
            let out = HwpxStructuralEditor::delete_paragraphs(
                &base,
                &[ParagraphLocator { section: 0, index: 1 }],
            )
            .unwrap();
            assert_eq!(
                RawPackage::read(&base).unwrap().read_entry("Contents/content.hpf").unwrap(),
                RawPackage::read(&out).unwrap().read_entry("Contents/content.hpf").unwrap()
            );
        } else {
            no_single_evidence(&base, 0);
        }
    }
}

// 이것을 실패시키는 것: section 비선형 허용 조건의 fallback 길이 제한 제거.
#[test]
fn nonlinear_sections_require_single_target() {
    let seed = extra(&script_seed(), "Scripts/opaque.bin", b"carry");
    for ids in [vec!["body-a"], vec!["body-b"], vec!["body-a", "body-b"]] {
        let mut base = seed.clone();
        for id in ids {
            base = replace_raw(
                &base,
                "Contents/content.hpf",
                &format!("idref=\"{id}\" linear=\"yes\""),
                &format!("idref=\"{id}\" linear=\"no\""),
            );
        }
        for section in [0, 1] {
            no_single_evidence(&base, section);
        }
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
    let seed = crate::HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &crate::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let good = section_no(&extra(&seed, "Scripts/opaque.bin", b"carry"));
    succeeds(&good);
    let bad = replace_raw(
        &good,
        "Contents/content.hpf",
        "idref=\"masterpage0\" linear=\"yes\"",
        "idref='masterpage0' linear='no'",
    );
    no_single_evidence(&bad, 0);
}

// 이것을 실패시키는 것: narrow 결과와 fallback 배열 전체의 순서 비교 제거.
#[test]
fn nonlinear_order_keeps_package_proofs() {
    nonlinear_header_does_not_authorize_reordered_sections();
    let good = section_no(&carry_base());
    for (part, from, to) in [
        ("Contents/content.hpf", "<opf:itemref idref=\"section0\" linear=\"no\"/>", ""),
        (
            "Contents/content.hpf",
            "</opf:spine>",
            "<opf:itemref idref='section0' linear='no'/></opf:spine>",
        ),
        ("Contents/content.hpf", "idref=\"section0\"", "idref='missing'"),
        ("Contents/content.hpf", "href=\"Contents/section0.xml\"", "href='Contents/missing.xml'"),
        ("Contents/content.hpf", "<opf:spine>", "<opf:spine future='1'>"),
        ("Contents/content.hpf", "idref=\"section0\"", "idref='section0' fallback='other'"),
        ("Contents/content.hpf", "http://www.idpf.org/2007/opf/", "urn:foreign"),
        ("META-INF/container.xml", "<ocf:rootfiles>", "<ocf:rootfiles future='1'>"),
        (
            "META-INF/container.xml",
            "urn:oasis:names:tc:opendocument:xmlns:container",
            "urn:foreign",
        ),
        ("META-INF/container.xml", "<ocf:rootfiles>", "<ocf:rootfiles systemLanguage='ko'>"),
    ] {
        no_single_evidence(&replace_raw(&good, part, from, to), 0);
    }
    let source =
        RawPackage::read(&good).unwrap().read_entry("Contents/section0.xml").unwrap().to_vec();
    no_single_evidence(&extra(&good, "Contents/section1.xml", &source), 0);
    let script = scripts(&good, "<opf:itemref idref='aux-h' linear='no'/>");
    succeeds(&script);
    for (from, to) in [
        ("Scripts/headerScripts.js", "Scripts/other.js"),
        ("id='aux-h' href='Scripts/headerScripts.js' media-type='application/x-javascript ;charset=utf-16'", "id='aux-h' href='Scripts/headerScripts.js' media-type='application/javascript'"),
        ("</opf:manifest>", "<opf:item id='alias' href='Scripts/headerScripts.js' media-type='application/xml'/></opf:manifest>"),
        ("</opf:manifest>", "<opf:item id='aux-h' href='Scripts/sourceScripts.js' media-type='application/xml'/></opf:manifest>"),
        ("</opf:spine>", "<opf:itemref idref='aux-h'/></opf:spine>"),
    ] {
        no_single_evidence(&replace_raw(&script, "Contents/content.hpf", from, to), 0);
    }
    let mut missing = RawPackage::read(&script).unwrap();
    missing.remove_entries_exact(&["Scripts/headerScripts.js".into()]);
    no_single_evidence(&missing.write().unwrap(), 0);
}

// 이것을 실패시키는 것: package 근거 생성에서 metadata 파싱 성공 조건 제거.
#[test]
fn single_section_metadata_proof_is_required() {
    let good = section_no(&carry_base());
    succeeds(&good);
    let broken = replace_raw(
        &good,
        "Contents/content.hpf",
        "<opf:title/>",
        "<opf:title><![CDATA[x]]></opf:title>",
    );
    assert_eq!(
        HwpxDecoder::decode(&broken).unwrap().warnings,
        HwpxDecoder::decode(&good).unwrap().warnings
    );
    no_single_evidence(&broken, 0);
    let dropped = replace_raw(
        &good,
        "Contents/content.hpf",
        "<opf:title/>",
        &format!("<opf:title>{}</opf:title>", "x".repeat(65537)),
    );
    assert_eq!(
        package_causes(&dropped),
        [crate::decoder::preserved_loss::LossCause::SingleSectionNonlinearOrder]
    );
    let d = HwpxDecoder::decode(&dropped).unwrap();
    assert!(d.warnings.iter().any(|w| matches!(w, crate::DecodeWarning::ContentLost { kind, .. } if kind != "package_reading_order")));
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs(
            &dropped,
            &[ParagraphLocator { section: 0, index: 1 }]
        )
        .unwrap_err(),
        original_error(&dropped)
    );
}

// 이것을 실패시키는 것: 새 package 원인 소비에서 단일 section 길이 방어 제거.
#[test]
fn single_section_evidence_rejects_multiple_sections() {
    use crate::decoder::preserved_loss::LossCause;
    let base = header_order(&script_seed());
    let (mut d, p, _) = admit_delete(&base).unwrap();
    preserved_loss::check_candidates(&base, &p, &d).unwrap();
    let losses = &mut std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap()).losses;
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].cause, LossCause::HeaderOnlyNonlinearOrder);
    let crate::decoder::preserved_loss::LossLocation::Package { sections } = &losses[0].location
    else {
        panic!()
    };
    assert_eq!(sections, &["Contents/section0.xml", "Contents/section1.xml"]);
    losses[0].cause = LossCause::SingleSectionNonlinearOrder;
    assert!(preserved_loss::check_candidates(&base, &p, &d).is_err());
}

// 이것을 실패시키는 것: evidence와 warning의 인덱스 동등 검사 제거.
#[test]
fn single_section_evidence_matches_source() {
    use crate::decoder::preserved_loss::LossLocation;
    let base = section_no(&scalar(&carry_base()));
    for change in [
        "missing",
        "duplicate",
        "extra",
        "index",
        "kind",
        "part",
        "sections",
        "order",
        "sha",
        "absent",
        "location",
    ] {
        let (mut d, p, _) = admit_delete(&base).unwrap();
        preserved_loss::check_candidates(&base, &p, &d).unwrap();
        let basis = std::sync::Arc::make_mut(d.source_basis.as_mut().unwrap());
        assert_eq!(basis.losses.len(), 2);
        match change {
            "missing" => {
                basis.losses.pop();
            }
            "duplicate" => basis.losses[1] = basis.losses[0].clone(),
            "extra" => basis.losses.push(basis.losses[1].clone()),
            "index" => basis.losses[1].warning = 99,
            "kind" | "part" => {
                let crate::DecodeWarning::ContentLost { kind, part, .. } =
                    &mut d.warnings[basis.losses[1].warning]
                else {
                    panic!()
                };
                if change == "kind" {
                    *kind = "unknown".into();
                } else {
                    *part = "Contents/section0.xml".into();
                }
            }
            "sections" => {
                let LossLocation::Package { sections } = &mut basis.losses[1].location else {
                    panic!()
                };
                sections[0] = "Contents/section7.xml".into();
            }
            "order" => basis.losses.swap(0, 1),
            "location" => basis.losses[1].location = basis.losses[0].location.clone(),
            "absent" | "sha" => {}
            _ => unreachable!(),
        }
        if change == "absent" {
            d.source_basis = None;
        }
        let different = section_no(&carry_base());
        let input = if change == "sha" { &different } else { &base };
        assert!(preserved_loss::check_candidates(input, &p, &d).is_err(), "{change}");
    }
    let (mut d, p, _) = admit_delete(&base).unwrap();
    for w in &mut d.warnings {
        let crate::DecodeWarning::ContentLost { reason, .. } = w else { panic!() };
        *reason = "unrelated opaque diagnostic".into();
    }
    preserved_loss::check_candidates(&base, &p, &d).unwrap();
    d.warnings.push(crate::DecodeWarning::ContentLost {
        part: "Contents/section0.xml".into(),
        path: Default::default(),
        kind: "unknown_element".into(),
        reason: "unrelated opaque diagnostic".into(),
    });
    assert!(preserved_loss::check_candidates(&base, &p, &d).is_err());
}

// 이것을 실패시키는 것: 재시도의 단일 비선형 section 허용 비트를 끄기.
#[test]
fn single_section_other_losses_stay_scoped() {
    for base in [
        scalar(&carry_base()),
        inline(&carry_base(), "남음"),
        body_object(&carry_base(), "남음", &shape()),
    ] {
        succeeds(&section_no(&base));
    }
    let outline = replace_raw(
        &outline_seed(1, 2),
        "Contents/section0.xml",
        "outlineShapeIDRef=\"1\"",
        "outlineShapeIDRef='2'",
    );
    let table = body_object(
        &replace(&table_base(), "Contents/section0.xml", "<hp:t>칸", "<hp:t>앞칸"),
        "칸",
        &shape(),
    );
    for bad in [
        inline(&carry_base(), "가"),
        body_object(&carry_base(), "가 &amp; &lt;나&gt;끝", &shape()),
        table,
        replace_raw(
            &carry_base(),
            "Contents/section0.xml",
            "<hp:t>남음",
            "<hp:unknown/><hp:t>남음",
        ),
        outline,
    ] {
        let bad = section_no(&bad);
        assert_eq!(
            HwpxStructuralEditor::delete_paragraphs(
                &bad,
                &[ParagraphLocator { section: 0, index: 1 }]
            )
            .unwrap_err(),
            original_error(&bad)
        );
    }
}

// 이것을 실패시키는 것: 보존 삭제 내부 오류를 원래 admission 오류로 복원하는 경계 제거.
#[test]
fn single_section_failure_keeps_admission_error() {
    let base = section_no(&carry_base());
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
        let actual = HwpxStructuralEditor::delete_paragraphs(
            &base,
            &[ParagraphLocator { section: 0, index: 1 }],
        )
        .unwrap_err();
        assert_eq!(actual, expected);
        assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
        assert_eq!(actual.to_string(), expected.to_string());
        assert!(visits().contains(&gate));
    }
    failures(&[]);
}

// 이것을 실패시키는 것: 새 section 근거가 있는 stamp 입력의 공통 손실 거부 생략.
#[test]
fn single_section_other_editors_and_reencode_unchanged() {
    let mut successes = [0; 4];
    let field = editor_control(&generated(), "editor control");
    for (seed, has_loss, expected) in [
        (header_order(&generated()), true, [false, true, false, true]),
        (scalar(&header_order(&generated())), true, [false, true, false, true]),
        (header_order(&field), true, [false, true, true, true]),
        (field, false, [true, true, true, true]),
    ] {
        let base = if has_loss {
            section_no(&seed)
        } else {
            replace_raw(
                &seed,
                "Contents/content.hpf",
                "</opf:metadata>",
                "</opf:metadata><!--editor control-->",
            )
        };
        let control = seed;
        let d = HwpxDecoder::decode(&base).unwrap();
        let c = HwpxDecoder::decode(&control).unwrap();
        assert_eq!(d.document, c.document);
        assert_eq!(d.style_store, c.style_store);
        assert_eq!(d.image_store, c.image_store);
        assert_eq!(d.warnings, c.warnings);
        let reencode = |d: &HwpxDocument| {
            crate::HwpxEncoder::encode_with_diagnostics(
                &d.document.clone().validate().unwrap(),
                &d.style_store,
                &d.image_store,
                crate::EncodeOptions::default(),
            )
            .unwrap()
        };
        let a = reencode(&d);
        let b = reencode(&c);
        assert_eq!(a.bytes, b.bytes);
        assert_eq!(a.warnings, b.warnings);
        let content_preserved = |output: &[u8], input: &[u8]| {
            if has_loss {
                assert_eq!(
                    RawPackage::read(output).unwrap().read_entry("Contents/content.hpf").unwrap(),
                    RawPackage::read(input).unwrap().read_entry("Contents/content.hpf").unwrap()
                );
            }
        };
        let normalize = |path: &str, bytes: &[u8]| {
            if path == "Contents/content.hpf" {
                if has_loss {
                    assert_eq!(bytes, RawPackage::read(&base).unwrap().read_entry(path).unwrap());
                }
                std::str::from_utf8(bytes)
                    .unwrap()
                    .replace(
                        "idref=\"section0\" linear=\"no\"",
                        "idref=\"section0\" linear=\"yes\"",
                    )
                    .replace("<!--editor control-->", "")
                    .into_bytes()
            } else {
                bytes.to_vec()
            }
        };
        assert_editor_result(
            ("stamp", expected[0], &mut successes[0]),
            &base,
            &control,
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&base, &[]),
            crate::stamp::HwpxStamper::stamp_with_diagnostics(&control, &[]),
            normalize,
            |r, input| {
                content_preserved(&r.bytes, input);
                assert_eq!(r.manifest.source_sha256, crate::patch::sha256_hex(input));
                assert_eq!(r.manifest.output_sha256, crate::patch::sha256_hex(&r.bytes));
                (
                    &r.bytes,
                    (
                        r.manifest.schema_version,
                        serde_json::to_value(&r.manifest.fields).unwrap(),
                        r.outcome.clone(),
                    ),
                )
            },
        );
        let patch = |input: &[u8], d: &HwpxDocument| {
            let mut section = d.document.sections()[0].clone();
            section.paragraphs[1] = paragraph("edited\u{1}");
            let p = crate::HwpxPatcher::export_section_preservation(
                input,
                0,
                &d.document.sections()[0],
            )
            .unwrap();
            crate::HwpxPatcher::patch_section_preserving_with_codec_diagnostics(
                input,
                0,
                &section,
                None,
                Some(&p),
            )
        };
        assert_editor_result(
            ("patch", expected[1], &mut successes[1]),
            &base,
            &control,
            patch(&base, &d),
            patch(&control, &c),
            normalize,
            |bytes, input| {
                content_preserved(bytes, input);
                (bytes, ())
            },
        );
        let fills = [("field".into(), "value".into())].into();
        assert_editor_result(
            ("fill", expected[2], &mut successes[2]),
            &base,
            &control,
            crate::HwpxFiller::fill_with_codec_diagnostics(&base, &fills),
            crate::HwpxFiller::fill_with_codec_diagnostics(&control, &fills),
            normalize,
            |r, input| {
                content_preserved(&r.bytes, input);
                (&r.bytes, serde_json::to_value(&r.filled).unwrap())
            },
        );
        assert_editor_result(
            ("insert", expected[3], &mut successes[3]),
            &base,
            &control,
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &base,
                ParagraphLocator { section: 0, index: 1 },
                InsertPosition::After,
                &["insert".into()],
            ),
            HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
                &control,
                ParagraphLocator { section: 0, index: 1 },
                InsertPosition::After,
                &["insert".into()],
            ),
            normalize,
            |bytes, input| {
                content_preserved(bytes, input);
                (bytes, ())
            },
        );
    }
    assert_eq!(successes, [1, 4, 2, 4], "stamp/patch/fill/insert success counts");
    assert!(successes.into_iter().all(|count| count >= 1));
}

// 이것을 실패시키는 것: 흐름 변화 중 Unknown 각주·미주 정책을 허용.
#[test]
fn single_section_linear_controls_keep_derived_and_bytes() {
    #[derive(Clone, Copy, Debug)]
    enum Expect {
        Allow,
        Derived,
        Height,
        Source,
    }
    let number = |kind| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='DIGIT' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    let mut cases = vec![];
    for (label, content, expect) in [
        ("body PAGE", number("PAGE"), Expect::Allow),
        ("body TOTAL_PAGE", number("TOTAL_PAGE"), Expect::Allow),
        ("unknown number", number("UNKNOWN"), Expect::Derived),
        ("unknown format", number("PAGE").replace("type='DIGIT'", "type='UNKNOWN'"), Expect::Derived),
        ("footer total", format!("<hp:ctrl><hp:footer><hp:subList><hp:p><hp:run>{}</hp:run></hp:p></hp:subList></hp:footer></hp:ctrl>", number("TOTAL_PAGE")), Expect::Derived),
    ] {
        cases.push((label.to_owned(), replace_raw(&carry_base(), "Contents/section0.xml", "<hp:t>남음</hp:t>", &content), expect, false));
    }
    for note in ["footNote", "endNote"] {
        for policy in ["CONTINUOUS", "ON_SECTION", "ON_PAGE", "UNKNOWN", "missing"] {
            for changed in [false, true] {
                let seed = carry_base();
                let source = RawPackage::read(&seed)
                    .unwrap()
                    .read_text_entry("Contents/section0.xml")
                    .unwrap();
                let start = source.find(&format!("<hp:{note}Pr>")).unwrap();
                let end = source[start..].find(&format!("</hp:{note}Pr>")).unwrap()
                    + start
                    + format!("</hp:{note}Pr>").len();
                let pr = &source[start..end];
                let numbering_start = pr.find("<hp:numbering ").unwrap();
                let numbering_end = pr[numbering_start..].find("/>").unwrap() + numbering_start + 2;
                let old_numbering = &pr[numbering_start..numbering_end];
                let new_numbering = match policy {
                    "missing" => String::new(),
                    _ => format!("<hp:numbering type='{policy}' newNum='1'/>"),
                };
                let new_pr = pr.replacen(old_numbering, &new_numbering, 1);
                let seed = replace_raw(&seed, "Contents/section0.xml", pr, &new_pr);
                let note_xml = format!("<hp:ctrl><hp:{note} number='1'><hp:subList><hp:p><hp:run><hp:t>주석</hp:t></hp:run></hp:p></hp:subList></hp:{note}></hp:ctrl>");
                let seed =
                    replace_raw(&seed, "Contents/section0.xml", "<hp:t>남음</hp:t>", &note_xml);
                let seed = if changed {
                    replace_raw(
                        &seed,
                        "Contents/section0.xml",
                        "<hp:t>가 &amp; &lt;나&gt;끝</hp:t>",
                        &note_xml,
                    )
                } else {
                    seed
                };
                let expect = if matches!(policy, "CONTINUOUS" | "ON_SECTION") && !changed {
                    Expect::Allow
                } else {
                    Expect::Derived
                };
                cases.push((format!("{note} {policy} changed={changed}"), seed, expect, false));
            }
        }
    }
    for (kind, expect) in [
        ("OBJECT_TYPE_PAGE", Expect::Allow),
        ("OBJECT_TYPE_NUMBER", Expect::Derived),
        ("OBJECT_TYPE_CONTENTS", Expect::Derived),
    ] {
        let content = format!("<hp:ctrl><hp:bookmark name='mark'/><hp:fieldBegin id='10' type='CROSSREF' dirty='0'><hp:parameters><hp:stringParam name='RefType'>TARGET_BOOKMARK</hp:stringParam><hp:stringParam name='RefContentType'>{kind}</hp:stringParam><hp:stringParam name='RefPath'>?mark;</hp:stringParam><hp:stringParam name='Command'>?mark;6;0;0;0;</hp:stringParam></hp:parameters></hp:fieldBegin></hp:ctrl><hp:t>3</hp:t><hp:ctrl><hp:fieldEnd beginIDRef='10'/></hp:ctrl>");
        cases.push((
            kind.into(),
            replace_raw(&carry_base(), "Contents/section0.xml", "<hp:t>남음</hp:t>", &content),
            expect,
            false,
        ));
    }
    cases.push(("supported table".into(), table_base(), Expect::Allow, true));
    let mut unsupported = RawPackage::read(&table_base()).unwrap();
    let source = unsupported.read_text_entry("Contents/section0.xml").unwrap();
    assert_eq!(source.matches("treatAsChar=\"0\"").count(), 2);
    unsupported.replace_text_entry(
        "Contents/section0.xml",
        source.replace("treatAsChar=\"0\"", "treatAsChar=\"1\""),
    );
    cases.push(("unsupported table".into(), unsupported.write().unwrap(), Expect::Height, true));
    cases.push((
        "unsafe deleted source".into(),
        replace_raw(&carry_base(), "Contents/section0.xml", "<hp:t>가", "<!--unknown--><hp:t>가"),
        Expect::Source,
        false,
    ));
    for (label, seed, expect, table) in cases {
        for input in [seed.clone(), section_no(&seed)] {
            let d = HwpxDecoder::decode(&input).unwrap();
            let p = RawPackage::read(&input).unwrap();
            let f = crate::derived_values::DerivedFacts::collect(&p, &d).unwrap();
            let ordinals = [f.body_ordinal(0, 1).unwrap()];
            let effects = f
                .effects(&[crate::derived_values::DerivedEdit::Delete {
                    section: 0,
                    paragraphs: &ordinals,
                }])
                .unwrap();
            assert_eq!(
                effects.check_derived_values().is_ok(),
                !matches!(expect, Expect::Derived),
                "{label}"
            );
            assert_eq!(
                effects.collect_height_invalidations().is_ok(),
                !matches!(expect, Expect::Height),
                "{label}"
            );
            let result = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(
                &input,
                &[ParagraphLocator { section: 0, index: 1 }],
            );
            match expect {
                Expect::Allow => {
                    let out = result.unwrap_or_else(|e| panic!("{label}: {e}"));
                    assert_eq!(out.decode_warnings, d.warnings);
                    let after = RawPackage::read(&out.value).unwrap();
                    assert_eq!(
                        p.read_entry("Contents/content.hpf").unwrap(),
                        after.read_entry("Contents/content.hpf").unwrap()
                    );
                    let mut expected = d.document;
                    expected.sections_mut()[0].paragraphs.remove(1);
                    let mut actual = HwpxDecoder::decode(&out.value).unwrap().document;
                    expected.strip_layout_caches();
                    actual.strip_layout_caches();
                    assert_eq!(actual, expected, "{label}");
                    let source =
                        std::str::from_utf8(after.read_entry("Contents/section0.xml").unwrap())
                            .unwrap();
                    if table {
                        assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 1);
                        assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 1);
                        assert_eq!(source.matches(CACHE).count(), 3);
                        for index in [0, 2, 3] {
                            let before_span = &d.source_maps[0].paragraphs[index].outer;
                            let before_source =
                                std::str::from_utf8(p.read_entry("Contents/section0.xml").unwrap())
                                    .unwrap();
                            let before_paragraph = &before_source[before_span.clone()];
                            if index == 0 {
                                assert!(source.contains(before_paragraph));
                            } else {
                                assert!(!source.contains(before_paragraph));
                            }
                        }
                    } else {
                        let original =
                            std::str::from_utf8(p.read_entry("Contents/section0.xml").unwrap())
                                .unwrap();
                        let span = &d.source_maps[0].paragraphs[1].outer;
                        assert_eq!(
                            source,
                            format!("{}{}", &original[..span.start], &original[span.end..])
                        );
                    }
                }
                Expect::Derived | Expect::Height | Expect::Source => {
                    let actual = result.unwrap_err();
                    let expected = original_error(&input);
                    assert_eq!(actual, expected, "{label}");
                    assert_eq!(format!("{actual:?}"), format!("{expected:?}"));
                    assert_eq!(actual.to_string(), expected.to_string());
                }
            }
        }
    }
    let item = "<opf:item id='prv' href='Preview/PrvText.txt' media-type='text/plain'/>";
    let root = "<ocf:rootfile full-path='Preview/PrvText.txt' media-type='text/plain'/>";
    for seed in [carry_base(), section_no(&carry_base())] {
        for descriptors in [false, true] {
            let base = extra(
                &extra(&seed, "Preview/PrvText.txt", b"preview"),
                "Preview/PrvImage.png",
                b"image",
            );
            let base = if descriptors {
                let base = replace_raw(
                    &base,
                    "Contents/content.hpf",
                    "</opf:manifest>",
                    &format!("{item}</opf:manifest>"),
                );
                replace_raw(
                    &base,
                    "META-INF/container.xml",
                    "</ocf:rootfiles>",
                    &format!("{root}</ocf:rootfiles>"),
                )
            } else {
                base
            };
            let d = HwpxDecoder::decode(&base).unwrap();
            let out = HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(
                &base,
                &[ParagraphLocator { section: 0, index: 1 }],
            )
            .unwrap();
            assert_eq!(out.decode_warnings, d.warnings);
            assert_eq!(out.encode_warnings.len(), 1);
            assert_eq!(out.encode_warnings[0].code(), "PREVIEW_REMOVED");
            let before = RawPackage::read(&base).unwrap();
            let after = RawPackage::read(&out.value).unwrap();
            assert!(!after.entry_paths().any(|p| p.starts_with("Preview/")));
            for (part, removal) in
                [("Contents/content.hpf", item), ("META-INF/container.xml", root)]
            {
                let source = std::str::from_utf8(before.read_entry(part).unwrap()).unwrap();
                assert_eq!(
                    after.read_entry(part).unwrap(),
                    if descriptors { source.replacen(removal, "", 1) } else { source.to_owned() }
                        .as_bytes()
                );
            }
            let changes = FINAL_CHANGES.with(|c| c.borrow().clone());
            let preview = crate::raw_package::preview::PreviewRemoval::plan(&before).unwrap();
            preview.verify_final(&out.value).unwrap();
            let mut expected = d.document.clone();
            expected.sections_mut()[0].paragraphs.remove(1);
            for (part, from, to) in [
                (
                    "Contents/content.hpf",
                    "idref=\"header\" linear=\"yes\"",
                    "idref=\"header\" linear=\"Yes\"",
                ),
                ("Contents/section0.xml", "outlineShapeIDRef=\"1\"", "outlineShapeIDRef=\"0\""),
            ] {
                let altered = replace_raw(&out.value, part, from, to);
                let original = RawPackage::read(&out.value).unwrap();
                let tampered = RawPackage::read(&altered).unwrap();
                let original = original.read_entry(part).unwrap();
                let tampered = tampered.read_entry(part).unwrap();
                assert_eq!(original.len(), tampered.len());
                assert_eq!(original.iter().zip(tampered).filter(|(a, b)| a != b).count(), 1);
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
                    .and_then(|bytes| preview.verify_final(&bytes).map_err(verification_error))
                    .is_err(),
                    "{part}"
                );
            }
        }
    }
}
