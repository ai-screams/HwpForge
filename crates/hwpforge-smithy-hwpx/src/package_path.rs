//! One canonical part-name policy for package input and output.
use crate::error::{HwpxError, HwpxResult};

/// Valid names are already canonical: never silently delete path components.
/// No filesystem-dependent interpretation (in particular Windows drive paths).
pub(crate) fn normalize_part_name(name: &str) -> HwpxResult<&str> {
    // 이것을 실패시키는 것: 위험한 성분을 삭제해 서로 다른 패키지 파트를 합치기.
    let drive = name.as_bytes().get(1) == Some(&b':')
        && name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if drive
        || name.contains('\\')
        || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(HwpxError::InvalidStructure {
            detail: format!("invalid package part name '{name}'"),
        });
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이것을 실패시키는 것: 빈 이름/경로 성분·점·역슬래시·드라이브 검사 중 하나 삭제.
    #[test]
    fn zip9_fix1_part_name_policy() {
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
