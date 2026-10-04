//! C4: the encoder checks the package it writes — manifest ids two items
//! share, and `binaryItemIDRef` values no image or OLE item resolves. The
//! checks only report; the bytes stay as they are.

use std::io::Read;

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeOutcome, EncodeWarning, HwpxEncoder};

fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

fn encode_runs(runs: Vec<Run>, images: &ImageStore) -> EncodeOutcome {
    let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        images,
        EncodeOptions::default(),
    )
    .expect("encode")
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut text = String::new();
    zip.by_name(name).expect("part").read_to_string(&mut text).expect("utf-8");
    text
}

fn conflicts(warnings: &[EncodeWarning]) -> Vec<(String, Vec<String>)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::PackageIdConflict { id, hrefs } => Some((id.clone(), hrefs.clone())),
            _ => None,
        })
        .collect()
}

/// An image stored as `header.png` gets the manifest id `header`, which the
/// header part already has. Reported, bytes unchanged (both items written).
///
/// 이것을 실패시키는 것: `encode_with_diagnostics` 에서
/// `manifest_id_conflicts` 호출을 빼는 것.
#[test]
fn an_image_stored_as_header_png_is_a_package_id_conflict() {
    let mut images = ImageStore::new();
    images.insert("header.png", vec![0x89, b'P', b'N', b'G']);
    let outcome = encode_runs(vec![Run::text("본문", CharShapeIndex::new(0))], &images);

    assert_eq!(
        conflicts(&outcome.warnings),
        [(
            "header".to_string(),
            vec!["Contents/header.xml".to_string(), "BinData/header.png".to_string()]
        )]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
    let hpf = part(&outcome.bytes, "Contents/content.hpf");
    assert_eq!(hpf.matches(r#"<opf:item id="header""#).count(), 2, "bytes are not rewritten");
}

#[test]
fn distinct_image_keys_are_not_a_conflict() {
    let mut images = ImageStore::new();
    images.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
    images.insert("image2.jpg", vec![0xFF, 0xD8]);
    let outcome = encode_runs(vec![Run::text("본문", CharShapeIndex::new(0))], &images);
    assert_eq!(conflicts(&outcome.warnings), []);
}
