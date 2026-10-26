use super::*;

/// MD → HWPX → styled MD (G1→G2→G3).
pub(super) fn md_roundtrip(markdown: &str) -> String {
    let template = builtin_default().expect("builtin");
    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode");
    let bridge = HwpxRegistryBridge::from_registry(&md_doc.style_registry).expect("bridge");
    let rebound = bridge.rebind_draft_document(md_doc.document).expect("rebind");
    let validated = rebound.validate().expect("validate");
    let bytes = HwpxEncoder::encode(
        &validated,
        bridge.style_store(),
        &hwpforge_core::image::ImageStore::new(),
    )
    .expect("HWPX encode");
    let doc = HwpxDecoder::decode(&bytes).expect("HWPX decode");
    let validated = doc.document.validate().expect("validate2");
    let lookup = HwpxStyleLookup::new(&doc.style_store, &doc.image_store);
    MdEncoder::encode_styled(&validated, &lookup).markdown
}
