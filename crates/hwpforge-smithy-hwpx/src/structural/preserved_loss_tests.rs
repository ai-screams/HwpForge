use super::*;

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
