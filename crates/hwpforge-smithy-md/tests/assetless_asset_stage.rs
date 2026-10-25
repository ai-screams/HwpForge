//! A Hancom assetless picture frame (`binaryItemIDRef=""`, Core path `""`)
//! survives both public asset entry points — `load_referenced_images` and
//! `collect_asset_plan` → `resolve_files_from_dir` → `finish_assets` —
//! whether the document came from the HWPX decoder or from lossless
//! Markdown: the frame stays, nothing is requested for it and no warning
//! names it.
//!
//! 이것을 실패시키는 것: `collect_asset_plan`·`apply` 의 빈 경로 건너뛰기 삭제
//! (빈 틀이 `File("")` 로 계획되어 "넣지 못함" 경고와 함께 지워진다).

use hwpforge_core::{Document, RunContent};
use hwpforge_smithy_hwpx::HwpxDecoder;
use hwpforge_smithy_md::assets::fs::resolve_files_from_dir;
use hwpforge_smithy_md::{
    collect_asset_plan, finish_assets, load_referenced_images, warnings_from, MdDecoder, MdEncoder,
    MdWarning,
};

fn hwpx_document() -> Document {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/images/f6-dangling-image-resaved.hwpx"
    ))
    .unwrap();
    let mut doc = Document::new();
    for section in HwpxDecoder::decode(&bytes).unwrap().document.sections() {
        doc.add_section(section.clone());
    }
    doc
}

fn lossless_document() -> Document {
    let md = MdEncoder::encode_lossless(&hwpx_document().validate().unwrap()).unwrap();
    MdDecoder::decode_lossless(&md).unwrap()
}

fn empty_frames(doc: &Document) -> usize {
    let mut n = 0;
    doc.for_each_paragraph(|p| {
        n += p
            .runs
            .iter()
            .filter(|r| matches!(&r.content, RunContent::Image(i) if i.path.is_empty()))
            .count();
    });
    n
}

fn names_empty(warnings: &[MdWarning]) -> bool {
    warnings.iter().any(|w| matches!(w, MdWarning::ImageEmbedSkipped { src, .. } if src.is_empty()))
}

#[test]
fn assetless_frames_survive_both_asset_entry_points() {
    for (origin, source) in [("hwpx", hwpx_document()), ("lossless", lossless_document())] {
        let frames = empty_frames(&source);
        assert_eq!(frames, 1, "{origin}: the fixture holds one assetless frame");
        for base in [None, Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))] {
            let mut doc = source.clone();
            let embedded = load_referenced_images(&mut doc, base);
            assert_eq!(empty_frames(&doc), frames, "{origin} {base:?}: load_referenced_images");
            assert!(!names_empty(&embedded.warnings), "{origin} {base:?}: {:?}", embedded.warnings);

            let doc = source.clone();
            let plan = collect_asset_plan(&doc);
            assert!(
                plan.iter().all(|e| !e.source.as_src().is_empty()),
                "{origin}: no asset request for the frame: {plan:?}"
            );
            let provided = resolve_files_from_dir(&plan, base);
            let finished = finish_assets(doc, &plan, provided).unwrap();
            assert_eq!(empty_frames(&finished.document), frames, "{origin} {base:?}: staged");
            let warnings = warnings_from(&plan, &finished.outcomes).unwrap();
            assert!(!names_empty(&warnings), "{origin} {base:?}: {warnings:?}");
        }
    }
}

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
