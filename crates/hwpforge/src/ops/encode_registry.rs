//! Shared registry projection and HWPX encoding backend.
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::{
    document::{Document, Draft},
    image::ImageStore,
};
use hwpforge_smithy_hwpx::{
    EncodeOptions, EncodeWarning, HancomStyleSet, HwpxEncoder, HwpxRegistryBridge,
    RegistryBridgeWarning,
};

#[cfg(any(feature = "ops-md", test))]
use hwpforge_smithy_hwpx::ListHeadPolicy;

use super::OpsError;

pub(crate) struct RegistryEncoded {
    pub bytes: Vec<u8>,
    pub sections: usize,
    pub paragraphs: usize,
    pub bridge_warnings: Vec<RegistryBridgeWarning>,
    pub encode_warnings: Vec<EncodeWarning>,
}

#[cfg(any(feature = "ops-md", test))]
pub(crate) fn encode_registry(
    document: Document<Draft>,
    registry: &StyleRegistry,
    images: &ImageStore,
    list_policy: ListHeadPolicy,
) -> Result<RegistryEncoded, OpsError> {
    let bridge = HwpxRegistryBridge::from_registry_with_diagnostics(
        registry,
        HancomStyleSet::default(),
        list_policy,
    )
    .map_err(OpsError::style_store)?;
    encode_projected(document, images, bridge)
}

#[cfg(feature = "ops-docx")]
pub(crate) fn encode_import_registry(
    document: Document<Draft>,
    registry: &StyleRegistry,
    images: &ImageStore,
) -> Result<RegistryEncoded, OpsError> {
    let bridge = HwpxRegistryBridge::from_registry_for_import(registry, HancomStyleSet::default())
        .map_err(OpsError::style_store)?;
    encode_projected(document, images, bridge)
}

fn encode_projected(
    document: Document<Draft>,
    images: &ImageStore,
    bridge: hwpforge_smithy_hwpx::WithBridgeWarnings<HwpxRegistryBridge>,
) -> Result<RegistryEncoded, OpsError> {
    let rebound = bridge.value.rebind_draft_document(document).map_err(OpsError::style_rebind)?;
    let validated = rebound.validate()?;
    let sections = validated.sections().len();
    let paragraphs = validated.sections().iter().map(|s| s.paragraphs.len()).sum();
    let encoded = HwpxEncoder::encode_with_diagnostics(
        &validated,
        bridge.value.style_store(),
        images,
        EncodeOptions::default(),
    )
    .map_err(OpsError::encode)?;
    Ok(RegistryEncoded {
        bytes: encoded.bytes,
        sections,
        paragraphs,
        bridge_warnings: bridge.warnings,
        encode_warnings: encoded.warnings,
    })
}

#[cfg(all(test, feature = "ops-docx"))]
#[path = "../../tests/support/docx.rs"]
pub(super) mod docx;

#[cfg(all(test, feature = "ops-docx"))]
mod tests {
    use super::*;
    use hwpforge_core::run::RunContent;
    use hwpforge_foundation::FontId;

    // 이것을 실패시키는 것: list_policy 무시, 원본 글꼴 preset 교체, bridge/encode 경고 폐기.
    #[test]
    fn source_fonts_policies_and_each_stage_diagnostic_survive() {
        for (policy, first_head) in
            [(ListHeadPolicy::Template, "^1."), (ListHeadPolicy::PreserveEffective, "")]
        {
            let mut decoded = hwpforge_smithy_docx::decode_docx(&docx::package(
                "<w:p><w:r><w:t>본문</w:t></w:r></w:p>",
            ))
            .unwrap();
            decoded.registry.fonts = vec![FontId::new("Source Typeface").unwrap()];
            for shape in &mut decoded.registry.char_shapes {
                shape.font = "Source Typeface".into();
                shape.font_latin = None;
                shape.font_east_asian = None;
            }
            decoded.registry.char_shapes[0].font_latin = Some("Missing Typeface".into());
            decoded.document.sections_mut()[0].paragraphs[0].runs[0].content =
                RunContent::Text("본문\u{1}".into());
            let encoded =
                encode_registry(decoded.document, &decoded.registry, &decoded.image_store, policy)
                    .unwrap();
            assert_eq!((encoded.sections, encoded.paragraphs), (1, 1));
            assert_eq!(
                encoded.bridge_warnings,
                [RegistryBridgeWarning::FontFallback {
                    requested_font: "Missing Typeface".into(),
                    slot: "latin",
                    char_shape: 0,
                    count: 1
                }]
            );
            assert_eq!(encoded.encode_warnings.len(), 1);
            assert_eq!(encoded.encode_warnings[0].code(), "XML_FORBIDDEN_CHARS_REMOVED");
            let roundtrip = hwpforge_smithy_hwpx::HwpxDecoder::decode(&encoded.bytes).unwrap();
            assert_eq!(roundtrip.document.sections()[0].paragraphs[0].text_content(), "본문");
            assert_eq!(
                roundtrip.style_store.iter_numberings().next().unwrap().levels[0].text,
                first_head
            );
            assert!(roundtrip
                .style_store
                .iter_fonts()
                .any(|font| font.face_name == "Source Typeface"));
        }
    }
}
