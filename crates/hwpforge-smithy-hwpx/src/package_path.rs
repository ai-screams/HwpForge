//! One canonical part-name policy for package input and output.
use crate::error::{HwpxError, HwpxResult};

/// Valid names are already canonical: never silently delete path components.
/// No filesystem-dependent interpretation (in particular Windows drive paths).
pub(crate) fn normalize_part_name(name: &str) -> HwpxResult<&str> {
    hwpforge_core::zip_package::normalize_part_name(name).map_err(package_error)
}

/// Preserve the existing HWPX error variants at the shared package boundary.
pub(crate) fn package_error(error: hwpforge_core::CoreError) -> HwpxError {
    match error {
        hwpforge_core::CoreError::InvalidStructure { context, reason } if context == "ZIP" => {
            HwpxError::Zip(reason)
        }
        hwpforge_core::CoreError::InvalidStructure { reason, .. } => {
            HwpxError::InvalidStructure { detail: reason }
        }
        other => HwpxError::Core(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이것을 실패시키는 것: 빈 이름/경로 성분·점·역슬래시·드라이브 검사 중 하나 삭제.
    #[test]
    fn part_name_policy() {
        for name in ["", ".", "..", "a/./b", "a/../b", "a//b", "a/", "/a", "a\\b", "C:/a", "C:a"] {
            assert!(
                matches!(normalize_part_name(name), Err(HwpxError::InvalidStructure { .. })),
                "{name}"
            );
        }
        for name in [
            "mimetype",
            "BinData/한글.png",
            "Chart/café.xml",
            "Contents/section0.xml",
            "a/b/c",
            "a..png",
            ".hidden",
        ] {
            assert_eq!(normalize_part_name(name).unwrap(), name);
        }
    }
}
