#![allow(dead_code)]

#[path = "fixtures.rs"]
pub(super) mod fixtures;

use hwpforge_convert::ops::{ConvertOpsWarning, ToPdfOptions};
use std::path::PathBuf;

/// The committed synthetic faces smithy-pdf renders its own e2e with.
pub(super) fn test_font_dir() -> PathBuf {
    fixtures::test_font_dir()
}

/// Render with the committed test fonts in degraded mode.
pub(super) fn options() -> ToPdfOptions {
    fatal_options().with_degraded(true)
}

/// The same fonts with the default (fatal) failure mode.
pub(super) fn fatal_options() -> ToPdfOptions {
    ToPdfOptions::default().with_font_dirs(vec![test_font_dir()])
}

pub(super) fn codes(warnings: &[ConvertOpsWarning]) -> Vec<String> {
    warnings.iter().map(|w| w.info().code).collect()
}
