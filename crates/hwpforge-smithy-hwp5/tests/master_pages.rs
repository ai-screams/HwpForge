use hwpforge_smithy_hwp5::{
    build_hwp5_semantic, decode_hwp5_to_core, Hwp5SemanticContainerKind,
    Hwp5SemanticMasterPageScope,
};
use hwpforge_smithy_hwpx::HwpxDecoder;
use std::{fs, path::PathBuf};

// 이것을 실패시키는 것: secd 목록을 본문에 남기기 / 홀짝 순서·특수 쪽·앞/겹침 플래그 변경 / master 그림 누락.
#[test]
fn master_pages_match_hancom_scope_and_content() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/master_pages");
    for name in [
        "f5a-last-page",
        "f5b-optional-page3",
        "f5c-odd-even",
        "f5d-overlap",
        "f5e-front",
        "f5f-shapes",
    ] {
        let bytes = fs::read(root.join(format!("{name}.hwp"))).unwrap();
        let decoded = decode_hwp5_to_core(&bytes).unwrap();
        let reference =
            HwpxDecoder::decode(&fs::read(root.join(format!("{name}.hwpx"))).unwrap()).unwrap();
        let masters = decoded.document.sections()[0].master_pages.as_ref().unwrap();
        let expected = reference.document.sections()[0].master_pages.as_ref().unwrap();
        assert_eq!(masters.len(), expected.len(), "{name}");
        for (actual, expected) in masters.iter().zip(expected) {
            assert_eq!(
                Hwp5SemanticMasterPageScope::from(actual),
                Hwp5SemanticMasterPageScope::from(expected),
                "{name}"
            );
            assert_eq!(actual.paragraphs.len(), expected.paragraphs.len(), "{name}");
            assert_eq!(
                actual.paragraphs.iter().map(|p| p.text_content()).collect::<Vec<_>>(),
                expected.paragraphs.iter().map(|p| p.text_content()).collect::<Vec<_>>(),
                "{name}"
            );
        }
        assert_eq!(decoded.master_layout_hints[0].len(), masters.len());
        let semantic = build_hwp5_semantic(&bytes).unwrap();
        assert!(semantic.graph_is_coherent(), "{name}: {:?}", semantic.graph_integrity_issues());
        assert_eq!(semantic.sections[0].master_pages.len(), masters.len());
        for master in &semantic.sections[0].master_pages {
            assert!(semantic.sections[0].paragraphs.iter().any(|p| p.container.segments
                == [Hwp5SemanticContainerKind::MasterPageSubList(master.index)]));
        }
        if name == "f5f-shapes" {
            assert!(masters
                .iter()
                .flat_map(|m| &m.paragraphs)
                .flat_map(|p| &p.runs)
                .any(|r| matches!(r.content, hwpforge_core::run::RunContent::Image(_))));
            assert!(!decoded.image_store.is_empty());
        }
    }
}
