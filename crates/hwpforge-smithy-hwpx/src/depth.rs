//! Shared section-index correction for Core depth errors.

use hwpforge_core::{CoreError, CoreResult, Section};

/// Checks semantic depth and reports the section's index in its document.
// 이것을 실패시키는 것: 깊이 검사 생략 또는 색인 보정 삭제 — decode_depth 및 iterative_walkers.
pub(crate) fn check_section_depth(section: &Section, index: usize) -> CoreResult<()> {
    section.check_nesting_depth().map_err(|error| match error {
        CoreError::InvalidStructure { context, reason } => CoreError::InvalidStructure {
            context,
            reason: reason.replace("section=0", &format!("section={index}")),
        },
        other => other,
    })
}
