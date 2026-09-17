//! `ops::convert::{decode_md, convert_md}` — Markdown in, HWPX out, and
//! the asset contract in between.
//!
//! `convert_md` is the only operation allowed to reach the filesystem, and
//! only by handing `base_dir` to `ops::fs`. These tests exercise all four
//! ways an image reference can end: embedded from a file, embedded from an
//! inline `data:` URI, dropped because the file is missing, and dropped
//! because there was no base directory to resolve against.
#![cfg(feature = "ops-md")]

use std::path::{Path, PathBuf};

use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::AssetOutcome;
use hwpforge::ops::convert::{convert_md, decode_md, ConvertMdOptions};

/// The smallest valid PNG: a 1×1 opaque pixel.
const ONE_PIXEL_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

/// A private directory under cargo's per-target scratch space.
///
/// `CARGO_TARGET_TMPDIR` is cargo's own answer to "where may an integration
/// test write?", so the crate needs no temp-directory dependency.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

fn codes(warnings: &[hwpforge::ops::OpsWarning]) -> Vec<String> {
    warnings.iter().map(|w| w.info().code).collect()
}

// ── decode_md ───────────────────────────────────────────────────

#[test]
fn plain_markdown_decodes_with_an_empty_asset_plan() {
    let decoded =
        decode_md("# 제목\n\n본문입니다.\n", &ConvertMdOptions::default()).expect("decode");

    assert!(!decoded.document.sections().is_empty());
    assert!(decoded.plan.is_empty(), "no images referenced: {:?}", decoded.plan);
    assert!(decoded.warnings.is_empty());
}

#[test]
fn the_asset_plan_lists_one_entry_per_image_run_in_document_order() {
    let markdown = "![first](a.png)\n\n본문\n\n![second](b.png)\n";

    let decoded = decode_md(markdown, &ConvertMdOptions::default()).expect("decode");

    assert_eq!(decoded.plan.len(), 2, "{:?}", decoded.plan);
    let first = decoded.plan[0].occurrence;
    let second = decoded.plan[1].occurrence;
    assert!(
        (first.paragraph, first.run) < (second.paragraph, second.run),
        "the plan must be in document order: {first:?} then {second:?}"
    );
}

#[test]
fn decode_rejects_an_unknown_preset_before_parsing_anything() {
    let err = decode_md("# 제목", &ConvertMdOptions::default().with_preset("modern"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
}

#[test]
fn unclosed_frontmatter_is_a_markdown_decode_failure() {
    let err =
        decode_md("---\ntitle: 제목\n", &ConvertMdOptions::default()).expect_err("must reject");

    assert_eq!(err.code(), OpsCode::MdDecodeFailed, "{err}");
}

// ── convert_md ──────────────────────────────────────────────────

#[test]
fn markdown_without_images_converts_to_a_decodable_package() {
    let out =
        convert_md("# 제목\n\n본문입니다.\n", None, &ConvertMdOptions::default()).expect("convert");

    let decoded = HwpxDecoder::decode(&out.bytes).expect("the output must decode");
    assert_eq!(decoded.document.sections().len(), 1);
    assert!(out.assets.is_empty());
}

#[test]
fn a_relative_image_next_to_the_base_dir_is_embedded() {
    let dir = scratch("convert_md_embeds");
    std::fs::write(dir.join("logo.png"), ONE_PIXEL_PNG).expect("write image");

    let out = convert_md("![로고](logo.png)\n", Some(&dir), &ConvertMdOptions::default())
        .expect("convert");

    assert_eq!(out.assets.len(), 1, "{:?}", out.assets);
    let AssetOutcome::Embedded { key, .. } = &out.assets[0] else {
        panic!("the image should have embedded: {:?}", out.assets);
    };
    assert!(key.ends_with(".png"), "the synthetic key keeps the sniffed format: {key}");
    assert!(
        !codes(&out.warnings).iter().any(|c| c == "IMAGE_EMBED_SKIPPED"),
        "a successful embed is not a warning: {:?}",
        codes(&out.warnings)
    );
    HwpxDecoder::decode(&out.bytes).expect("the output must decode");
}

#[test]
fn a_missing_image_is_dropped_and_reported_exactly_once() {
    let dir = scratch("convert_md_missing");

    let out = convert_md("![없음](missing.png)\n", Some(&dir), &ConvertMdOptions::default())
        .expect("convert");

    assert!(matches!(out.assets.as_slice(), [AssetOutcome::Dropped { .. }]), "{:?}", out.assets);
    let skipped: Vec<String> =
        codes(&out.warnings).into_iter().filter(|c| c == "IMAGE_EMBED_SKIPPED").collect();
    assert_eq!(skipped.len(), 1, "one exclusion, one warning: {:?}", codes(&out.warnings));
    HwpxDecoder::decode(&out.bytes).expect("a dropped image must not leave a dangling reference");
}

#[test]
fn an_inline_data_uri_embeds_without_any_base_dir() {
    use base64_shim::encode;
    let markdown = format!("![픽셀](data:image/png;base64,{})\n", encode(ONE_PIXEL_PNG));

    let out = convert_md(&markdown, None, &ConvertMdOptions::default()).expect("convert");

    assert!(
        matches!(out.assets.as_slice(), [AssetOutcome::Embedded { .. }]),
        "the bytes are already in the document: {:?}",
        out.assets
    );
    HwpxDecoder::decode(&out.bytes).expect("the output must decode");
}

#[test]
fn without_a_base_dir_a_relative_reference_is_excluded_not_guessed() {
    // Resolving it against the process's working directory would read a
    // file the caller never named.
    let out =
        convert_md("![로고](logo.png)\n", None, &ConvertMdOptions::default()).expect("convert");

    assert!(matches!(out.assets.as_slice(), [AssetOutcome::Dropped { .. }]), "{:?}", out.assets);
}

#[test]
fn a_remote_url_is_never_fetched() {
    let out = convert_md(
        "![원격](https://example.invalid/logo.png)\n",
        None,
        &ConvertMdOptions::default(),
    )
    .expect("convert");

    assert!(matches!(out.assets.as_slice(), [AssetOutcome::Remote { .. }]), "{:?}", out.assets);
}

#[test]
fn an_unknown_preset_is_refused() {
    let err = convert_md("# 제목", None, &ConvertMdOptions::default().with_preset("gov_proposal"))
        .expect_err("must reject");

    assert_eq!(err.code(), OpsCode::PresetNotFound, "{err}");
    assert!(err.to_string().contains("gov_proposal"), "{err}");
}

#[test]
fn no_markdown_can_reach_the_two_style_stages_as_a_failure() {
    // `STYLE_STORE_FAILED` and `STYLE_REBIND_FAILED` are wired, but neither
    // is reachable *from this operation*: the store is built from the same
    // registry `decode_md` resolved, and the document's shape indices come
    // from that same decode, so the two can never disagree. The preset gate
    // (only `"default"`) is what keeps the registry invariant.
    //
    // Constructs chosen to allocate as many distinct char/para shapes as the
    // decoder will: headings push para shapes, code blocks and blockquotes
    // push both, and inline emphasis pushes char shapes.
    let documents = [
        "# 제목\n\n## 부제\n\n### 소제목\n\n본문.\n",
        "- 하나\n- 둘\n  - 중첩\n\n1. 첫째\n2. 둘째\n",
        "```rust\nfn main() {}\n```\n\n> 인용문\n\n본문 **굵게** *기울임* ~~취소~~.\n",
        "| 머리 | 글 |\n| -- | -- |\n| 값 | 값 |\n\n본문.\n",
        "각주[^1] 와 미주[^e1].\n\n[^1]: 각주 본문\n\n[^e1]: 미주 본문\n",
        "---\ntitle: 제목\nauthor: 글쓴이\n---\n\n본문.\n",
    ];

    for markdown in documents {
        let out = convert_md(markdown, None, &ConvertMdOptions::default())
            .unwrap_or_else(|e| panic!("[{}] {e}", e.code().as_str()));
        HwpxDecoder::decode(&out.bytes)
            .expect("every style rebind must produce a readable package");
    }
}

#[test]
fn meta_carries_exactly_the_assets_and_warnings_keys() {
    let dir = scratch("convert_md_meta");
    std::fs::write(dir.join("logo.png"), ONE_PIXEL_PNG).expect("write image");
    let out = convert_md("![로고](logo.png)\n", Some(&dir), &ConvertMdOptions::default())
        .expect("convert");

    let value = serde_json::to_value(out.meta()).expect("serialise");
    let mut fields: Vec<&str> =
        value.as_object().expect("object").keys().map(String::as_str).collect();
    fields.sort_unstable();
    assert_eq!(fields, ["assets", "warnings"]);
    assert_eq!(value["assets"].as_array().expect("array").len(), 1);
}

/// base64 without a dependency — the test needs exactly one encode.
mod base64_shim {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    let index = (n >> (18 - 6 * i)) & 0x3F;
                    out.push(char::from(ALPHABET[index as usize]));
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    #[test]
    fn matches_the_known_encodings() {
        assert_eq!(encode(b""), "");
        assert_eq!(encode(b"f"), "Zg==");
        assert_eq!(encode(b"fo"), "Zm8=");
        assert_eq!(encode(b"foo"), "Zm9v");
        assert_eq!(encode(b"foob"), "Zm9vYg==");
        assert_eq!(encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(encode(b"foobar"), "Zm9vYmFy");
    }
}
