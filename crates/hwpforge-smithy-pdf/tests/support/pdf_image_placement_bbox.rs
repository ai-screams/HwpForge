use super::*;

// ── 렌더 경로 헬퍼 ─────────────────────────────────────────────────

pub(super) fn render_hwpx_bytes(
    hwpx_bytes: &[u8],
    options: &PdfOptions,
) -> (hwpforge_smithy_pdf::PdfOutput, Document<Validated>) {
    let decoded = HwpxDecoder::decode(hwpx_bytes).expect("hwpx decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let input = PdfInput { document: &validated, styles: &lookup };
    let output = render_document(&input, options).expect("render");
    (output, validated)
}
