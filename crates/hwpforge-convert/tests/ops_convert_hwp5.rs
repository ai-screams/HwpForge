use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options};
use hwpforge_foundation::diagnostics::OpsCode;

#[test]
fn the_option_builder_is_the_only_way_to_change_the_default() {
    assert!(ConvertHwp5Options::default().carry_layout_cache);
    assert!(!ConvertHwp5Options::default().with_carry_layout_cache(false).carry_layout_cache);
    assert!(
        ConvertHwp5Options::default()
            .with_carry_layout_cache(false)
            .with_carry_layout_cache(true)
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
