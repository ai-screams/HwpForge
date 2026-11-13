use hwpforge_core::{Document, ImageStore, LayoutProfile, PageSettings, Section};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxStyleStore};
#[path = "support/document.rs"]
mod document;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

const DEFAULT: &str = r#"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility/></hh:compatibleDocument>"#;

fn generated(profile: LayoutProfile) -> Vec<u8> {
    let mut doc = Document::new();
    doc.set_layout_profile(profile);
    doc.add_section(Section::with_paragraphs(
        vec![document::text_para("text")],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode(&doc.validate().unwrap(), &HwpxStyleStore::new(), &ImageStore::new())
        .unwrap()
}

fn roundtrip(bytes: &[u8]) -> Vec<u8> {
    let d = HwpxDecoder::decode(bytes).unwrap();
    HwpxEncoder::encode(&d.document.validate().unwrap(), &d.style_store, &d.image_store).unwrap()
}

#[test]
fn generated_profiles_have_independent_ordered_wire_contracts() {
    // 이것을 실패시키는 것: 모드 무시, 플래그 누락·재정렬·중복.
    let h = xml::Element::parse(&package::part(
        &generated(LayoutProfile::Hancom),
        "Contents/header.xml",
    ));
    let compat = h.child("hh:compatibleDocument");
    assert_eq!(compat.attr("targetProgram"), Some("HWP201X"));
    assert!(compat.child("hh:layoutCompatibility").children.is_empty());
    let bytes = generated(LayoutProfile::WordCompatible);
    let h = xml::Element::parse(&package::part(&bytes, "Contents/header.xml"));
    let compat = h.child("hh:compatibleDocument");
    assert_eq!(compat.attr("targetProgram"), Some("MS_WORD"));
    let names = compat.child("hh:layoutCompatibility").child_names();
    assert_eq!(
        names,
        [
            "hh:applyFontWeightToBold",
            "hh:useInnerUnderline",
            "hh:useLowercaseStrikeout",
            "hh:extendLineheightToOffset",
            "hh:treatQuotationAsLatin",
            "hh:doNotAlignWhitespaceOnRight",
            "hh:doNotAdjustWordInJustify",
            "hh:baseCharUnitOnEAsian",
            "hh:baseCharUnitOfIndentOnFirstChar",
            "hh:adjustLineheightToFont",
            "hh:adjustBaselineInFixedLinespacing",
            "hh:applyPrevspacingBeneathObject",
            "hh:applyNextspacingOfLastPara",
            "hh:adjustParaBorderfillToSpacing",
            "hh:connectParaBorderfillOfEqualBorder",
            "hh:adjustParaBorderOffsetWithBorder",
            "hh:extendLineheightToParaBorderOffset",
            "hh:applyParaBorderToOutside",
            "hh:applyMinColumnWidthTo1mm",
            "hh:applyTabPosBasedOnSegment",
            "hh:breakTabOverLine",
            "hh:adjustVertPosOfLine",
            "hh:doNotAlignLastForbidden",
            "hh:adjustMarginFromAdjustLineheight",
            "hh:baseLineSpacingOnLineGrid",
            "hh:applyCharSpacingToCharGrid",
            "hh:doNotApplyGridInHeaderFooter",
            "hh:applyExtendHeaderFooterEachSection",
            "hh:doNotApplyLinegridAtNoLinespacing",
            "hh:doNotAdjustEmptyAnchorLine",
            "hh:overlapBothAllowOverlap",
            "hh:extendVertLimitToPageMargins",
            "hh:doNotHoldAnchorOfTable",
            "hh:doNotFormattingAtBeneathAnchor",
            "hh:adjustBaselineOfObjectToBottom",
        ]
    );
    assert_eq!(names.iter().collect::<std::collections::BTreeSet<_>>().len(), 35);
    assert_eq!(
        HwpxDecoder::decode(&bytes).unwrap().document.layout_profile(),
        LayoutProfile::WordCompatible
    );
    assert_eq!(
        package::part(&roundtrip(&bytes), "Contents/header.xml"),
        package::part(&bytes, "Contents/header.xml")
    );
}

#[test]
fn source_compatibility_keeps_empty_partial_unknown_and_absent_trees() {
    // 이것을 실패시키는 것: 원본 대신 생성 상수 선택, 미지 프로그램 경고 누락.
    let base = generated(LayoutProfile::Hancom);
    for (raw, profile, unknown) in [
        ("", LayoutProfile::Hancom, false),
        (
            r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility/></hh:compatibleDocument>"#,
            LayoutProfile::WordCompatible,
            false,
        ),
        (
            r#"<hh:compatibleDocument targetProgram="MS_WORD" extra="a &amp; b"><hh:layoutCompatibility><hh:adjustLineheightToFont/><hh:future value="7"/></hh:layoutCompatibility></hh:compatibleDocument>"#,
            LayoutProfile::WordCompatible,
            false,
        ),
        (
            r#"<hh:compatibleDocument targetProgram="HWP201X"><hh:layoutCompatibility><hh:future/></hh:layoutCompatibility></hh:compatibleDocument>"#,
            LayoutProfile::Hancom,
            false,
        ),
        (
            r#"<hh:compatibleDocument targetProgram="FUTURE"><hh:layoutCompatibility><hh:future/></hh:layoutCompatibility></hh:compatibleDocument>"#,
            LayoutProfile::Hancom,
            true,
        ),
    ] {
        let bytes = package::splice_part(&base, "Contents/header.xml", DEFAULT, raw);
        let d = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(d.document.layout_profile(), profile);
        assert_eq!(d.warnings.iter().any(|w| matches!(w, hwpforge_smithy_hwpx::DecodeWarning::UnknownEnumValue {attribute: "hh:compatibleDocument@targetProgram", raw, fallback: "HWP201X"} if raw == "FUTURE")), unknown);
        assert_eq!(
            package::part(&roundtrip(&bytes), "Contents/header.xml"),
            package::part(&bytes, "Contents/header.xml")
        );
    }
}

#[test]
fn compatibility_extensions_keep_inherited_namespace_bindings() {
    // 이것을 실패시키는 것: 상위 선언 없이 원문 조각만 복사, 쓰지 않는 상위 prefix 보충.
    let bytes = package::splice_part(
        &generated(LayoutProfile::Hancom),
        "Contents/header.xml",
        "<hh:head ",
        r#"<hh:head xmlns:x="urn:future" xmlns:unused="urn:unused" "#,
    );
    let bytes = package::splice_part(
        &bytes,
        "Contents/header.xml",
        DEFAULT,
        r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility><x:flag x:value="7"/></hh:layoutCompatibility></hh:compatibleDocument>"#,
    );
    let out = roundtrip(&bytes);
    let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
    assert_eq!(h.child("hh:compatibleDocument").attr("xmlns:x"), Some("urn:future"));
    assert_eq!(h.child("hh:compatibleDocument").attr("xmlns:unused"), None);
    assert_eq!(h.find("x:flag").attr("x:value"), Some("7"));
    assert_eq!(roundtrip(&out), out);
}

#[test]
fn base_rejects_missing_or_different_profile_before_noop_and_preparation() {
    // 이것을 실패시키는 것: base 대조 누락, JSON 누락을 base 값으로 덮어쓰기.
    let base = generated(LayoutProfile::WordCompatible);
    let d = HwpxDecoder::decode(&base).unwrap();
    for changed in [false, true] {
        let mut value = serde_json::to_value(&d.document).unwrap();
        value.as_object_mut().unwrap().remove("layout_profile");
        let mut submitted: Document = serde_json::from_value(value).unwrap();
        if changed {
            submitted.sections_mut()[0].paragraphs[0] = document::text_para("edited");
        }
        let error = HwpxEncoder::encode_with_base(
            &base,
            &submitted.validate().unwrap(),
            &d.style_store,
            &d.image_store,
            EncodeOptions::default(),
        )
        .unwrap_err();
        assert!(
            matches!(error, hwpforge_smithy_hwpx::HwpxError::InvalidStructure { detail } if detail.contains("/layout_profile") && detail.contains("Hancom") && detail.contains("WordCompatible"))
        );
    }
}

#[test]
fn matching_json_edit_uses_base_partial_flags_and_general_change_regenerates() {
    // 이것을 실패시키는 것: 최초 출력 전에 base 문맥 복구 누락, 명시 변경에 원본 재사용.
    let raw = r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility><hh:useInnerUnderline/></hh:layoutCompatibility></hh:compatibleDocument>"#;
    let base = package::splice_part(
        &generated(LayoutProfile::Hancom),
        "Contents/header.xml",
        DEFAULT,
        raw,
    );
    let d = HwpxDecoder::decode(&base).unwrap();
    let mut submitted: Document =
        serde_json::from_value(serde_json::to_value(&d.document).unwrap()).unwrap();
    submitted.sections_mut()[0].paragraphs[0] = document::text_para("edited");
    let out = HwpxEncoder::encode_with_base(
        &base,
        &submitted.validate().unwrap(),
        &d.style_store,
        &d.image_store,
        EncodeOptions::default(),
    )
    .unwrap();
    let h = xml::Element::parse(&package::part(&out.bytes, "Contents/header.xml"));
    assert_eq!(h.find("hh:layoutCompatibility").child_names(), ["hh:useInnerUnderline"]);
    let mut doc = d.document;
    doc.set_layout_profile(LayoutProfile::Hancom);
    let out =
        HwpxEncoder::encode(&doc.validate().unwrap(), &d.style_store, &d.image_store).unwrap();
    let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
    assert_eq!(h.find("hh:compatibleDocument").attr("targetProgram"), Some("HWP201X"));
    assert!(h.find("hh:layoutCompatibility").children.is_empty());
}

#[test]
fn bridge_preserves_profile_and_private_source_for_both_document_states() {
    // 이것을 실패시키는 것: validated bridge의 프로필 또는 원본 문맥 복사 삭제.
    use hwpforge_blueprint::{registry::StyleRegistry, template::Template};
    use hwpforge_smithy_hwpx::HwpxRegistryBridge;
    let registry = StyleRegistry::from_template(&Template::from_yaml("meta:\n  name: bridge\nstyles:\n  body:\n    char_shape:\n      font: Base\n      size: 10pt\n").unwrap()).unwrap();
    let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
    let raw = r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility><hh:future/></hh:layoutCompatibility></hh:compatibleDocument>"#;
    let bytes = package::splice_part(
        &generated(LayoutProfile::Hancom),
        "Contents/header.xml",
        DEFAULT,
        raw,
    );
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    for validated in [false, true] {
        let doc = if validated {
            bridge.rebind_validated_document(&decoded.document.clone().validate().unwrap()).unwrap()
        } else {
            bridge.rebind_draft_document(decoded.document.clone()).unwrap().validate().unwrap()
        };
        assert_eq!(doc.layout_profile(), LayoutProfile::WordCompatible);
        let out = HwpxEncoder::encode(&doc, bridge.style_store(), &ImageStore::new()).unwrap();
        let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
        assert_eq!(h.find("hh:layoutCompatibility").child_names(), ["hh:future"]);
    }
}

#[test]
fn explicit_profile_change_invalidates_all_caches_even_when_requested() {
    // 이것을 실패시키는 것: 일반 출력의 변경된 모드에서 emit_layout_cache 허용.
    let bytes = generated(LayoutProfile::Hancom);
    let bytes = package::splice(
        &bytes,
        "</hp:p>",
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="10000" flags="393216"/></hp:linesegarray></hp:p>"#,
    );
    let mut d = HwpxDecoder::decode(&bytes).unwrap();
    assert!(d.document.sections()[0].paragraphs[0].layout_cache.is_some());
    d.document.set_layout_profile(LayoutProfile::WordCompatible);
    let out = HwpxEncoder::encode_with_diagnostics(
        &d.document.validate().unwrap(),
        &d.style_store,
        &d.image_store,
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    assert!(xml::Element::parse(&package::section_xml(&out.bytes))
        .descendants("hp:linesegarray")
        .is_empty());
}

#[test]
fn compatibility_local_namespaces_and_empty_element_are_preserved() {
    // 이것을 실패시키는 것: 자식 namespace 재선언 중복, self-closing 종료 위치 오류.
    for raw in [
        r#"<hh:compatibleDocument targetProgram="MS_WORD"/>"#,
        r#"<hh:compatibleDocument xmlns:x="urn:local" targetProgram="MS_WORD"><hh:layoutCompatibility><x:flag><![CDATA[a]]>&amp;&#32;</x:flag></hh:layoutCompatibility></hh:compatibleDocument>"#,
    ] {
        let bytes = package::splice_part(
            &generated(LayoutProfile::Hancom),
            "Contents/header.xml",
            "<hh:head ",
            r#"<hh:head xmlns:x="urn:ancestor" "#,
        );
        let bytes = package::splice_part(&bytes, "Contents/header.xml", DEFAULT, raw);
        let out = roundtrip(&bytes);
        let header = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
        let c = header.child("hh:compatibleDocument");
        assert_eq!(
            c.attr("xmlns:x"),
            if raw.contains("urn:local") { Some("urn:local") } else { None }
        );
        assert_eq!(roundtrip(&out), out);
    }
    let bytes = package::splice_part(
        &generated(LayoutProfile::Hancom),
        "Contents/header.xml",
        DEFAULT,
        &format!("{DEFAULT}{DEFAULT}"),
    );
    assert!(HwpxDecoder::decode(&bytes).is_err());
}

#[test]
fn inherited_namespaces_follow_element_and_attribute_scopes() {
    // 이것을 실패시키는 것: 속성 전용 prefix 누락, 자식 재선언을 상위 사용으로 오인, 빈 요소 scope 누출.
    for (children, x, y, default) in [
        (r#"<hh:flag x:value="7"/>"#, Some("urn:ancestor"), None, None),
        (r#"<hh:flag xmlns:x="urn:local"><x:flag/></hh:flag>"#, None, None, None),
        (r#"<hh:flag xmlns:x="urn:local"/><x:flag/>"#, Some("urn:ancestor"), None, None),
        (
            r#"<hh:flag xmlns:x="urn:local"><x:flag/></hh:flag><x:flag/>"#,
            Some("urn:ancestor"),
            None,
            None,
        ),
        (r#"<y:flag/>"#, None, Some("urn:other"), None),
        (r#"<flag/>"#, None, None, Some("urn:default")),
        (r#"<hh:flag value="7"/>"#, None, None, None),
    ] {
        let base = package::splice_part(
            &generated(LayoutProfile::Hancom),
            "Contents/header.xml",
            "<hh:head ",
            r#"<hh:head xmlns="urn:default" xmlns:x="urn:ancestor" xmlns:y="urn:other" "#,
        );
        let raw = format!(
            r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility>{children}</hh:layoutCompatibility></hh:compatibleDocument>"#
        );
        let bytes = package::splice_part(&base, "Contents/header.xml", DEFAULT, &raw);
        let out = roundtrip(&bytes);
        let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
        let c = h.child("hh:compatibleDocument");
        assert_eq!(c.attr("xmlns:x"), x, "{children}");
        assert_eq!(c.attr("xmlns:y"), y, "{children}");
        assert_eq!(c.attr("xmlns"), default, "{children}");
        assert_eq!(roundtrip(&out), out);
    }
}

#[test]
fn editing_surfaces_keep_base_profile_and_partial_flags() {
    // 이것을 실패시키는 것: 편집·검증 사본의 모드/원본 문맥 소실 또는 header 정규화 상수화.
    use hwpforge_core::{Paragraph, Run, Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::stamp::{plan, HwpxStamper, StampAction, StampRequestV2, StampSpec};
    use hwpforge_smithy_hwpx::{
        CellSpec, CellTarget, HwpxCellEditor, HwpxStructuralEditor, InsertPosition,
        ParagraphLocator,
    };
    use sha2::{Digest, Sha256};
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            document::text_para("name: (   )"),
            document::text_para("tail"),
            Paragraph::with_runs(
                vec![Run::table(
                    Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![document::text_para("cell")],
                        HwpUnit::new(8000).unwrap(),
                    )])]),
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            ),
        ],
        PageSettings::a4(),
    ));
    let specs = plan(&doc)
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "name".into(), hint: None },
        })
        .collect::<Vec<_>>();
    let bytes =
        HwpxEncoder::encode(&doc.validate().unwrap(), &HwpxStyleStore::new(), &ImageStore::new())
            .unwrap();
    let raw = r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility><hh:useInnerUnderline/></hh:layoutCompatibility></hh:compatibleDocument>"#;
    let base = package::splice_part(&bytes, "Contents/header.xml", DEFAULT, raw);
    let request = StampRequestV2 {
        schema_version: 2,
        source_sha256: Sha256::digest(&base).iter().map(|b| format!("{b:02x}")).collect(),
        text: specs.clone(),
        cells: vec![],
    };
    let mut outputs = vec![
        HwpxStamper::stamp(&base, &specs).unwrap().bytes,
        HwpxStamper::stamp_v2(&base, &request).unwrap().bytes,
    ];
    for text in ["same", "long\ncell\ntext"] {
        outputs.push(
            HwpxCellEditor::set_cells(
                &base,
                &[CellSpec {
                    table: 0,
                    target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 0)),
                    text: text.into(),
                }],
            )
            .unwrap()
            .bytes,
        );
    }
    let anchor = ParagraphLocator { section: 0, index: 1 };
    outputs.push(
        HwpxStructuralEditor::insert_paragraphs(
            &base,
            anchor,
            InsertPosition::After,
            &["inserted".into()],
        )
        .unwrap(),
    );
    outputs.push(HwpxStructuralEditor::delete_paragraphs(&base, &[anchor]).unwrap());
    for out in outputs {
        let d = HwpxDecoder::decode(&out).unwrap();
        assert_eq!(d.document.layout_profile(), LayoutProfile::WordCompatible);
        let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
        assert_eq!(h.find("hh:layoutCompatibility").child_names(), ["hh:useInnerUnderline"]);
    }
}

#[test]
fn compatibility_refuses_unbound_prefixes_entities_and_excessive_depth() {
    // 이것을 실패시키는 것: 원본 조각의 namespace/entity 검증·깊이 제한 생략.
    for child in [
        "<x:flag/>",
        "<hh:flag x:value='7'/>",
        "<hh:flag value='&missing;'/>",
        "<hh:flag>&missing;</hh:flag>",
        &format!("{}{}", "<hh:flag>".repeat(225), "</hh:flag>".repeat(225)),
    ] {
        let raw = format!(
            r#"<hh:compatibleDocument targetProgram="MS_WORD"><hh:layoutCompatibility>{child}</hh:layoutCompatibility></hh:compatibleDocument>"#
        );
        let bytes = package::splice_part(
            &generated(LayoutProfile::Hancom),
            "Contents/header.xml",
            DEFAULT,
            &raw,
        );
        assert!(HwpxDecoder::decode(&bytes).is_err(), "{child}");
    }
}
