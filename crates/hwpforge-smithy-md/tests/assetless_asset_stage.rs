#[path = "support/assetless_asset_stage.rs"]
mod shared;
use shared::*;

use hwpforge_core::RunContent;
use hwpforge_smithy_md::{load_referenced_images, MdDecoder, MdWarning};

/// The ordinary Markdown row: `![x]()` is the decoder's to report — one
/// warning, no picture — so the asset stage after it sees nothing and adds
/// no warning of its own, with or without a base directory.
/// 이것을 실패시키는 것: 디코더의 빈 대상 갈래 삭제(그림이 빈 틀로 남고 경고가
/// 없다).
#[test]
fn an_empty_markdown_target_is_reported_once_by_the_decoder() {
    for base in [None, Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))] {
        let decoded = MdDecoder::decode_with_default("# 제목\n\n![로고]()\n\n본문\n").unwrap();
        assert_eq!(decoded.warnings.len(), 1, "{base:?}: {:?}", decoded.warnings);
        assert!(names_empty(&decoded.warnings), "{base:?}: {:?}", decoded.warnings);
        let mut doc = decoded.document;
        let embedded = load_referenced_images(&mut doc, base);
        assert!(embedded.warnings.is_empty(), "{base:?}: {:?}", embedded.warnings);
        let mut pictures = 0;
        doc.for_each_paragraph(|p| {
            pictures += p.runs.iter().filter(|r| matches!(r.content, RunContent::Image(_))).count();
        });
        assert_eq!(pictures, 0, "{base:?}");
    }
}
