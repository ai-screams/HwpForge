use super::*;

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
// 이것을 실패시키는 것: delete-layout 체크포인트 제거.
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
        "derived-heading",
        "delete-layout",
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
// 이것을 실패시키는 것: 본문 쪽 번호 허용 또는 생존 캐시 제거 복원.
#[test]
fn single_section_linear_controls_keep_derived_and_bytes() {
    #[derive(Clone, Copy, Debug)]
    enum Expect {
        Allow,
        Derived,
        Source,
    }
    let number = |kind| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='DIGIT' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    let mut cases = vec![];
    for (label, content, expect) in [
        ("body PAGE", number("PAGE"), Expect::Derived),
        ("body TOTAL_PAGE", number("TOTAL_PAGE"), Expect::Derived),
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
        ("OBJECT_TYPE_PAGE", Expect::Derived),
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
    cases.push(("unsupported table".into(), unsupported.write().unwrap(), Expect::Allow, true));
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
            assert!(super::super::delete_layout::check(&p, &xml(&input)).is_ok(), "{label}");
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
                        assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 2);
                        assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 0);
                        assert_eq!(source.matches(CACHE).count(), 5);
                        for index in [0, 2, 3] {
                            let before_span = &d.source_maps[0].paragraphs[index].outer;
                            let before_source =
                                std::str::from_utf8(p.read_entry("Contents/section0.xml").unwrap())
                                    .unwrap();
                            let before_paragraph = &before_source[before_span.clone()];
                            assert!(source.contains(before_paragraph));
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
                Expect::Derived | Expect::Source => {
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
