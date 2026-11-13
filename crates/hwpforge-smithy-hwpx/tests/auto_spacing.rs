use hwpforge_blueprint::{
    registry::StyleRegistry,
    style::{ParaShape, PartialCharShape},
    EastAsianAutoSpacing,
};
use hwpforge_core::{
    Document, ImageStore, LayoutProfile, PageSettings, Paragraph, Run, Section, Validated,
};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    EncodeOptions, HwpxDecoder, HwpxEncoder, HwpxRegistryBridge, HwpxStyleStore, ListHeadPolicy,
};
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

fn fixture(
    values: &[Option<EastAsianAutoSpacing>],
    profile: LayoutProfile,
    policy: ListHeadPolicy,
) -> (Document<Validated>, HwpxStyleStore) {
    let mut registry: StyleRegistry = serde_json::from_value(serde_json::json!({"fonts":["맑은 고딕"],"char_shapes":[],"para_shapes":[],"style_entries":{}})).unwrap();
    let mut ch = PartialCharShape::default();
    ch.font = Some("맑은 고딕".into());
    ch.size = Some(hwpforge_foundation::HwpUnit::new(1100).unwrap());
    registry.char_shapes.push(ch.resolve("body").unwrap());
    let mut ps = vec![];
    for (i, value) in values.iter().enumerate() {
        let mut shape = ParaShape::default();
        shape.east_asian_auto_spacing = *value;
        registry.para_shapes.push(shape);
        ps.push(Paragraph::with_runs(
            vec![Run::text("한글 Latin 123", CharShapeIndex::new(0))],
            ParaShapeIndex::new(i),
        ));
    }
    let bridge =
        HwpxRegistryBridge::from_registry_with_diagnostics(&registry, Default::default(), policy)
            .unwrap()
            .value;
    let mut doc = Document::new();
    doc.set_layout_profile(profile);
    doc.add_section(Section::with_paragraphs(ps, PageSettings::a4()));
    let doc = bridge.rebind_draft_document(doc).unwrap().validate().unwrap();
    (doc, bridge.into_style_store())
}
fn encode(doc: &Document<Validated>, store: &HwpxStyleStore) -> Vec<u8> {
    HwpxEncoder::encode(doc, store, &ImageStore::new()).unwrap()
}
fn assert_values(bytes: &[u8], values: &[[u32; 2]]) {
    let h = xml::Element::parse(&package::part(bytes, "Contents/header.xml"));
    let s = xml::Element::parse(&package::section_xml(bytes));
    let shapes = h.descendants("hh:paraPr");
    let ps = s.descendants("hp:p");
    assert_eq!(ps.len(), values.len());
    for (p, [de, dn]) in ps.iter().zip(values) {
        let shape = shapes.iter().find(|s| s.attr("id") == p.attr("paraPrIDRef")).unwrap();
        assert_eq!(shape.children.iter().filter(|n| n.name == "hh:autoSpacing").count(), 1);
        let a = shape.child("hh:autoSpacing");
        assert_eq!(a.attr("eAsianEng"), Some(de.to_string().as_str()));
        assert_eq!(a.attr("eAsianNum"), Some(dn.to_string().as_str()));
    }
}
// 이것을 실패시키는 것: registry 위치를 출력 fidelity 키로 사용하거나 한 목록 정책에서 투영 누락.
#[test]
fn registry_auto_spacing_reaches_store_positions() {
    for policy in [ListHeadPolicy::Template, ListHeadPolicy::PreserveEffective] {
        let (doc, store) = fixture(
            &[
                Some(EastAsianAutoSpacing::new(false, true)),
                Some(EastAsianAutoSpacing::new(true, false)),
            ],
            LayoutProfile::Hancom,
            policy,
        );
        let v = serde_json::to_value(&store).unwrap();
        for (p, expected) in doc.sections()[0].paragraphs.iter().zip([[0, 1], [1, 0]]) {
            assert!(p.para_shape_id.get() > 1);
            assert_eq!(
                v["header_fidelity"]["auto_spacing"][p.para_shape_id.get().to_string()],
                serde_json::json!(expected)
            );
        }
        assert_values(&encode(&doc, &store), &[[0, 1], [1, 0]]);
    }
}
// 이것을 실패시키는 것: 프로필 기본값을 명시값보다 우선하거나 compatibility 변경.
#[test]
fn explicit_auto_spacing_overrides_layout_profile() {
    for (profile, setting, expected, target) in [
        (LayoutProfile::Hancom, true, [1, 1], "HWP201X"),
        (LayoutProfile::WordCompatible, false, [0, 0], "MS_WORD"),
    ] {
        let (doc, store) = fixture(
            &[Some(EastAsianAutoSpacing::new(setting, setting)), None],
            profile,
            ListHeadPolicy::Template,
        );
        let out = encode(&doc, &store);
        assert_values(&out, &[expected, if setting { [0, 0] } else { [1, 1] }]);
        let h = xml::Element::parse(&package::part(&out, "Contents/header.xml"));
        assert_eq!(h.child("hh:compatibleDocument").attr("targetProgram"), Some(target));
        assert_eq!(
            h.child("hh:compatibleDocument").child("hh:layoutCompatibility").children.len(),
            if setting { 0 } else { 35 }
        );
    }
}
// 이것을 실패시키는 것: 자동 간격만 있는 fidelity를 비었다고 판단하거나 JSON 경로 변경.
#[test]
fn auto_spacing_store_json_preserves_projected_values() {
    let (doc, store) = fixture(
        &[
            Some(EastAsianAutoSpacing::new(false, false)),
            Some(EastAsianAutoSpacing::new(false, true)),
            Some(EastAsianAutoSpacing::new(true, false)),
            Some(EastAsianAutoSpacing::new(true, true)),
        ],
        LayoutProfile::Hancom,
        ListHeadPolicy::PreserveEffective,
    );
    let mut v = serde_json::to_value(&store).unwrap();
    assert!(v["header_fidelity"]["auto_spacing"].is_object());
    assert!(v.get("east_asian_auto_spacing").is_none());
    let spacing = v["header_fidelity"]["auto_spacing"].clone();
    v["header_fidelity"] = serde_json::json!({"auto_spacing": spacing});
    let store: HwpxStyleStore = serde_json::from_value(v).unwrap();
    let json = serde_json::to_value(&store).unwrap();
    assert_eq!(json["header_fidelity"]["auto_spacing"], spacing);
    let store: HwpxStyleStore = serde_json::from_value(json).unwrap();
    assert_values(&encode(&doc, &store), &[[0, 0], [0, 1], [1, 0], [1, 1]]);
}
// 이것을 실패시키는 것: autoSpacing을 방출 XML 의존성에서 제외하거나 한 축 변경 누락.
#[test]
fn auto_spacing_changes_invalidate_layout_dependencies() {
    let (doc, store) = fixture(
        &[Some(EastAsianAutoSpacing::new(false, false))],
        LayoutProfile::Hancom,
        ListHeadPolicy::Template,
    );
    let bytes = package::splice(
        &encode(&doc, &store),
        "</hp:p>",
        r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="100" vertsize="1100" textheight="1100" baseline="850" spacing="600" horzpos="0" horzsize="10000" flags="393216"/></hp:linesegarray></hp:p>"#,
    );
    for value in [[0, 1], [1, 0]] {
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert!(decoded.document.sections()[0].paragraphs[0].layout_cache.is_some());
        let id = decoded.document.sections()[0].paragraphs[0].para_shape_id.get().to_string();
        let mut json = serde_json::to_value(&decoded.style_store).unwrap();
        json["header_fidelity"]["auto_spacing"][id] = serde_json::json!(value);
        let store: HwpxStyleStore = serde_json::from_value(json).unwrap();
        let doc = decoded.document.validate().unwrap();
        let out = HwpxEncoder::encode_with_diagnostics(
            &doc,
            &store,
            &decoded.image_store,
            EncodeOptions::default().with_emit_layout_cache(true),
        )
        .unwrap();
        assert_values(&out.bytes, &[value]);
        let base_analysis =
            hwpforge_smithy_hwpx::layout_edit::PackageAnalysis::read(&bytes).unwrap();
        let emitted_analysis =
            hwpforge_smithy_hwpx::layout_edit::PackageAnalysis::read(&out.bytes).unwrap();
        let plan = hwpforge_smithy_hwpx::layout_edit::plan_invalidation(
            &base_analysis,
            &emitted_analysis,
            &hwpforge_smithy_hwpx::layout_edit::ParagraphCorrespondence::identity(&base_analysis),
            &Default::default(),
        );
        let axis = if value[0] == 1 { "autoSpacing@eAsianEng" } else { "autoSpacing@eAsianNum" };
        assert!(plan.paragraphs[0].iter().any(|reason| reason.contains(axis)), "{plan:?}");
        let out = HwpxEncoder::encode_with_base(
            &bytes,
            &doc,
            &store,
            &decoded.image_store,
            EncodeOptions::default(),
        )
        .unwrap();
        assert!(xml::Element::parse(&package::section_xml(&out.bytes))
            .descendants("hp:linesegarray")
            .is_empty());
    }
}
