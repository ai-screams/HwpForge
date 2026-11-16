use super::*;

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

// 이것을 실패시키는 것: 개요 근거 없는 입력의 삭제 heading 검사 호출 제거.
fn outline_legacy_heading_control(base: &[u8]) {
    use crate::decoder::preserved_loss::LossCause;
    let input = scalar(base);
    let decoded = HwpxDecoder::decode(&input).unwrap();
    let losses = &decoded.source_basis.as_ref().unwrap().losses;
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].cause, LossCause::PerLanguageScalar);
    let targets = [ParagraphLocator { section: 0, index: 1 }];
    outline_check(&input, &targets, |_| {}).unwrap();
    assert_eq!(
        HwpxStructuralEditor::delete_paragraphs_with_codec_diagnostics(&input, &targets)
            .unwrap_err(),
        original_error(&input)
    );
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
// 이것을 실패시키는 것: 단일 대상 삭제 거부.
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
    for target in targets {
        HwpxStructuralEditor::delete_paragraphs(&base, &[target]).unwrap();
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
        outline_refused(&package.write().unwrap(), &[target]);
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
// 이것을 실패시키는 것: delete-layout 체크포인트 제거.
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
// 이것을 실패시키는 것: 생존 캐시 제거 복원.
#[test]
fn outline_tables_and_caches_stay_scoped() {
    let base = outline_input();
    let auto = |kind, format| {
        format!("<hp:ctrl><hp:autoNum num='0' numType='{kind}'><hp:autoNumFormat type='{format}' userChar='' prefixChar='' suffixChar='' supscript='0'/></hp:autoNum></hp:ctrl>")
    };
    for kind in ["PAGE", "TOTAL_PAGE"] {
        let good =
            replace(&base, "Contents/section0.xml", "<hp:t>0 survive</hp:t>", &auto(kind, "DIGIT"));
        outline_refused(&good, &[ParagraphLocator { section: 0, index: 1 }]);
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
    assert_eq!(source.matches("height=\"321\" heightRelTo=").count(), 2);
    assert_eq!(source.matches("height=\"0\" heightRelTo=").count(), 0);
    assert_eq!(source.matches("<hp:linesegarray>").count(), 5);
    let mut bad = RawPackage::read(&table).unwrap();
    bad.replace_text_entry(
        "Contents/section0.xml",
        xml(&table).replace("treatAsChar=\"0\"", "treatAsChar=\"1\""),
    );
    let bad = bad.write().unwrap();
    HwpxStructuralEditor::delete_paragraphs(&bad, &[ParagraphLocator { section: 0, index: 1 }])
        .unwrap();
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
// 이것을 실패시키는 것: 개요 검사 배치 계수 증가 제거.
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
        outline_check(&base, &targets, |_| {}).unwrap();
        assert_eq!(crate::derived_values::OUTLINE_BATCHES.with(std::cell::Cell::get), 1);
        let work = crate::work::WORK.with(std::cell::Cell::get);
        assert_eq!(crate::derived_values::COLLECTIONS.with(std::cell::Cell::get), 1);
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
