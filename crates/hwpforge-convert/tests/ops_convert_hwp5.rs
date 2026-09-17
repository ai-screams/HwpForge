//! `ops::convert_hwp5` end-to-end.
//!
//! The operation is a thin, shared face over
//! [`hwp5_to_hwpx_bytes_with_options`](hwpforge_convert::hwp5_to_hwpx_bytes_with_options),
//! which the crate's own suite already covers in depth. What these tests pin
//! is the part the frontends depend on and the library function does not have:
//! the option builder reaching the encoder, the stable error code, the
//! warning stage, and the shape of the `meta()` payload.

use std::io::Read;
use std::path::PathBuf;

use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options};
use hwpforge_foundation::diagnostics::OpsCode;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

/// Reads one entry out of an HWPX (ZIP) package as text.
fn entry_text(hwpx: &[u8], name: &str) -> String {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(hwpx)).expect("the output is a ZIP package");
    let mut entry = archive.by_name(name).unwrap_or_else(|err| panic!("entry {name}: {err}"));
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("entry is UTF-8 XML");
    text
}

// ── happy path ──────────────────────────────────────────────────

#[test]
fn an_hwp5_fixture_converts_to_an_hwpx_package() {
    let output =
        convert_hwp5(&fixture("pdf-rules/rules-header-multi.hwp"), &ConvertHwp5Options::default())
            .expect("convert");

    assert!(output.bytes.starts_with(b"PK"), "HWPX is a ZIP container");
    // The mimetype entry is what makes it an HWPX rather than any ZIP.
    assert_eq!(entry_text(&output.bytes, "mimetype"), "application/hwp+zip");

    // Every diagnostic from this operation belongs to the conversion stage.
    for warning in &output.warnings {
        assert_eq!(warning.stage(), "convert");
        let info = warning.info();
        assert!(!info.message.trim().is_empty(), "{} has a blank message", info.code);
        // Partial drift detection: whatever this fixture raises today is
        // classified. An upstream variant that lost its arm shows up here.
        assert_ne!(info.code, "OTHER", "unclassified convert warning: {}", info.message);
    }
}

#[test]
fn meta_carries_exactly_the_warnings() {
    let output =
        convert_hwp5(&fixture("pdf-rules/rules-header-multi.hwp"), &ConvertHwp5Options::default())
            .expect("convert");
    let meta = output.meta();

    assert_eq!(meta.warnings.len(), output.warnings.len());
    let value = serde_json::to_value(&meta).expect("serialise meta");
    let keys: Vec<&String> = value.as_object().expect("an object").keys().collect();
    assert_eq!(keys, vec!["warnings"], "the FFI key set is a contract");
}

// ── the one option ──────────────────────────────────────────────

#[test]
fn carry_layout_cache_decides_whether_line_segments_are_emitted() {
    // A Hancom-saved source, so there is a line-segment cache to carry.
    let source = fixture("pdf-rules/rules-bold.hwp");

    let off = convert_hwp5(&source, &ConvertHwp5Options::default()).expect("convert");
    assert!(!off.bytes.is_empty());
    assert!(
        !entry_text(&off.bytes, "Contents/section0.xml").contains("linesegarray"),
        "the default is an editing-safe package with no carried cache"
    );

    let on = convert_hwp5(&source, &ConvertHwp5Options::default().with_carry_layout_cache(true))
        .expect("convert");
    assert!(
        entry_text(&on.bytes, "Contents/section0.xml").contains("linesegarray"),
        "opting in carries the render material"
    );

    // Same document either way — only the cache differs.
    assert_ne!(off.bytes, on.bytes);
}

#[test]
fn the_option_builder_is_the_only_way_to_change_the_default() {
    assert!(!ConvertHwp5Options::default().carry_layout_cache);
    assert!(ConvertHwp5Options::default().with_carry_layout_cache(true).carry_layout_cache);
    assert!(
        !ConvertHwp5Options::default()
            .with_carry_layout_cache(true)
            .with_carry_layout_cache(false)
            .carry_layout_cache
    );
}

// ── failures ────────────────────────────────────────────────────

#[test]
fn bytes_that_are_not_an_hwp5_container_report_a_decode_failure() {
    let error = convert_hwp5(b"not a document at all", &ConvertHwp5Options::default())
        .expect_err("not an OLE2 container");

    assert_eq!(error.code(), OpsCode::Hwp5DecodeFailed);
    assert_eq!(error.code().as_str(), "HWP5_DECODE_FAILED");
    assert!(error.hint().is_some(), "the caller is told what to check");
    assert!(error.cause().is_none(), "a renderer code belongs to render failures only");
    assert!(!error.to_string().is_empty(), "the library message is kept, not normalised");
}

#[test]
fn an_hwpx_package_is_not_accepted_as_hwp5_input() {
    // `convert_hwp5` takes HWP5 only; content sniffing is `to_pdf`'s job.
    // A ZIP therefore fails at the container check rather than half-working.
    let error = convert_hwp5(&fixture("pdf-rules/rules-bold.hwpx"), &ConvertHwp5Options::default())
        .expect_err("a ZIP is not an OLE2 container");

    assert_eq!(error.code(), OpsCode::Hwp5DecodeFailed);
}

#[test]
fn a_truncated_container_is_refused_rather_than_partly_converted() {
    let mut truncated = fixture("pdf-rules/rules-header-multi.hwp");
    truncated.truncate(2_048);

    let error =
        convert_hwp5(&truncated, &ConvertHwp5Options::default()).expect_err("truncated source");
    assert_eq!(error.code(), OpsCode::Hwp5DecodeFailed);
}
