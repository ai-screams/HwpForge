//! Opening HWP5 or HWPX without changing the HWPX operation contract.
use super::{convert_hwp5, ConvertHwp5Options, ConvertOpsError, ConvertOpsWarning};
use hwpforge_core::{zip_package::open_package, CoreError};
use serde::{Deserialize, Serialize};

const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
use std::borrow::Cow;

/// Conversion options used when an input requires HWP5 conversion.
pub type OpenOptions = ConvertHwp5Options;

/// The format selected by the shared input classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    /// HWPX or other bytes left for the existing HWPX reader to diagnose.
    Hwpx,
    /// A CFB input whose ZIP central directory could not be read.
    Hwp5,
}

/// One conversion warning with its location kept separate from its message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionWarning {
    /// Pipeline stage.
    pub stage: String,
    /// Stable warning code.
    pub code: String,
    /// Human-readable message without the location prefix.
    pub message: String,
    /// Source location, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}
impl From<&ConvertOpsWarning> for ConversionWarning {
    fn from(warning: &ConvertOpsWarning) -> Self {
        let parts = warning.parts();
        Self {
            stage: warning.stage().into(),
            code: parts.code.into(),
            message: parts.message,
            location: parts.location,
        }
    }
}

/// Additive metadata, separate from an operation's existing warnings.
#[derive(Debug, Serialize)]
pub struct ConversionMeta {
    /// Original input format (`hwp5`).
    pub source_format: &'static str,
    /// Conversion diagnostics in pipeline order.
    pub warnings: Vec<ConversionWarning>,
}

/// Diagnostics from opening an HWP5 input as HWPX.
#[derive(Debug)]
pub struct ConversionReport {
    /// Typed warnings, retaining the format owners' loss classification.
    pub warnings: Vec<ConvertOpsWarning>,
}
impl ConversionReport {
    /// Metadata for frontends, without changing their document payload.
    #[must_use]
    pub fn meta(&self) -> ConversionMeta {
        ConversionMeta {
            source_format: "hwp5",
            warnings: self.warnings.iter().map(ConversionWarning::from).collect(),
        }
    }

    /// Reject unaccepted loss after editing succeeds and before writing bytes.
    ///
    /// # Errors
    /// [`ConvertOpsError::ConversionLoss`] contains only the rejected warnings.
    pub fn ensure_accepted(&self, accept: bool) -> Result<(), ConvertOpsError> {
        // 이것을 실패시키는 것: 명시적 허용 무시 또는 손실 거부 우회.
        if accept {
            return Ok(());
        }
        let warnings: Vec<_> = self
            .warnings
            .iter()
            .filter(|w| w.is_semantic_loss())
            .map(ConversionWarning::from)
            .collect();
        if warnings.is_empty() {
            Ok(())
        } else {
            Err(ConvertOpsError::ConversionLoss { warnings })
        }
    }
}

/// Original bytes without copying, or newly converted HWPX bytes.
#[derive(Debug)]
pub struct OpenedHwpx<'a> {
    /// Bytes to pass to the existing HWPX operation.
    pub bytes: Cow<'a, [u8]>,
    /// Classification used for input overwrite protection.
    pub source: SourceFormat,
    /// Present only when HWP5 conversion took place.
    pub conversion: Option<ConversionReport>,
}

/// Convert only inputs identified as HWP5 by the ZIP container failure and CFB signature.
///
/// Borrowed HWPX stays borrowed; owned HWPX keeps its allocation. Invalid
/// non-CFB input is left unchanged so argument errors still precede decoding.
///
/// # Errors
/// The same decode, validation, encode and conversion errors as [`convert_hwp5`].
pub fn open_as_hwpx<'a>(
    bytes: impl Into<Cow<'a, [u8]>>,
    options: &OpenOptions,
) -> Result<OpenedHwpx<'a>, ConvertOpsError> {
    let bytes = bytes.into();
    // 이것을 실패시키는 것: 변환 분기 우회 또는 정상 ZIP까지 변환.
    if bytes.starts_with(&CFB_MAGIC)
        && matches!(open_package(&bytes, Default::default()),
            Err(CoreError::InvalidStructure { context, reason })
                if context == "ZIP" && reason == "invalid ZIP central directory framing")
    {
        let converted = convert_hwp5(&bytes, options)?;
        Ok(OpenedHwpx {
            bytes: Cow::Owned(converted.bytes),
            source: SourceFormat::Hwp5,
            conversion: Some(ConversionReport { warnings: converted.warnings }),
        })
    } else {
        Ok(OpenedHwpx { bytes, source: SourceFormat::Hwpx, conversion: None })
    }
}
