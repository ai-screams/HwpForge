//! Input-only recovery classification. Call at the original input reader,
//! never around a generated output's decode or the entire operation.

use hwpforge_core::zip_package::open_package;
use hwpforge_core::CoreError;
use hwpforge_foundation::OpsCode;

use super::OpsError;

/// OLE2/CFB signature used only to advise callers after decoding fails.
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

pub(super) fn classify_decode_failure(bytes: &[u8], error: OpsError) -> OpsError {
    // 이것을 실패시키는 것: DecodeFailed 단계 조건 삭제 또는 공용 판별 우회.
    if error.code() == OpsCode::DecodeFailed && is_hwp5_input(bytes) {
        OpsError::Hwp5Input { source: Box::new(error) }
    } else {
        error
    }
}

/// Whether bytes need HWP5 conversion before an HWPX operation.
///
/// A CFB signature alone is insufficient: prefixed ZIP packages remain HWPX.
/// Only the ZIP reader's central-directory framing failure qualifies. Package
/// policy and XML errors retain their original classification. Non-CFB inputs
/// never invoke the ZIP reader, preserving argument-error precedence.
#[must_use]
fn is_hwp5_input(bytes: &[u8]) -> bool {
    // 이것을 실패시키는 것: CFB 조건 삭제, ZIP 실패 조건 삭제, 정책 오류까지 재분류.
    bytes.starts_with(&CFB_MAGIC)
        && matches!(open_package(bytes, Default::default()),
            Err(CoreError::InvalidStructure { context, reason })
                if context == "ZIP" && reason == "invalid ZIP central directory framing")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_smithy_hwpx::{stamp::StamperError, CodecFailure, CodecStage, HwpxError};

    #[test]
    // 이것을 실패시키는 것: classify_decode_failure의 DecodeFailed 단계 조건 삭제.
    fn non_input_stages_keep_their_original_code_and_message() {
        for stage in [
            CodecStage::Admission,
            CodecStage::Validation,
            CodecStage::Encode,
            CodecStage::Verification,
        ] {
            let cause = HwpxError::InvalidStructure { detail: "original cause".into() };
            let error = OpsError::from(StamperError::Codec(CodecFailure {
                stage,
                code: cause.code(),
                detail: cause.to_string(),
            }));
            let code = error.code();
            let message = error.to_string();
            let classified = classify_decode_failure(&CFB_MAGIC, error);
            assert_eq!(classified.code(), code);
            assert_eq!(classified.to_string(), message);
        }
    }
}
