//! HWP5 → HWPX conversion as an operation: [`convert_hwp5`].

use hwpforge_foundation::diagnostics::WarningInfo;
use serde::{Deserialize, Serialize};

use super::{ConvertOpsError, ConvertOpsWarning};
use crate::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};

/// Options for [`convert_hwp5`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConvertHwp5Options {
    /// Emit the HWP5 line-segment cache into the HWPX output as
    /// `<hp:linesegarray>`. Default `false`.
    ///
    /// This is opt-in because the carried cache is **render material, not an
    /// editing surface**: the output is meant for the PDF replay pipeline,
    /// not for reopening in Hancom Office. Leave it off unless the bytes feed
    /// a renderer. [`super::to_pdf`] turns it on for its own HWP5 leg.
    pub carry_layout_cache: bool,
}

impl ConvertHwp5Options {
    /// Sets whether the layout cache is carried into the HWPX output.
    #[must_use]
    pub const fn with_carry_layout_cache(mut self, carry: bool) -> Self {
        self.carry_layout_cache = carry;
        self
    }
}

/// What [`convert_hwp5`] returns: the HWPX package and what was lost on the
/// way.
#[derive(Debug)]
#[non_exhaustive]
pub struct ConvertHwp5Output {
    /// The converted HWPX package.
    pub bytes: Vec<u8>,
    /// Decode, projection, style-mapping and encode diagnostics, in that
    /// order. Every one is at stage `"convert"`.
    pub warnings: Vec<ConvertOpsWarning>,
}

impl ConvertHwp5Output {
    /// The serialisable wire payload; the bytes travel beside it.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::ConvertHwp5Output;
    ///
    /// # fn show(output: &ConvertHwp5Output) {
    /// let meta = output.meta();
    /// assert_eq!(meta.warnings.len(), output.warnings.len());
    /// # }
    /// ```
    #[must_use]
    pub fn meta(&self) -> Hwp5Meta {
        Hwp5Meta { warnings: self.warnings.iter().map(ConvertOpsWarning::info).collect() }
    }
}

/// The `convert_hwp5` wire payload: `{ "warnings": [ … ] }`.
///
/// # Serde
///
/// `Serialize` and `Deserialize`, plus `JsonSchema` under the `schemars`
/// feature — the same contract the umbrella's `*Meta` wrappers carry, so a
/// frontend can describe every operation's payload with one flag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Hwp5Meta {
    /// One entry per conversion warning, in pipeline order.
    pub warnings: Vec<WarningInfo>,
}

/// Converts HWP5 bytes to HWPX bytes.
///
/// This is the operation-layer face of
/// [`hwp5_to_hwpx_bytes_with_options`](crate::hwp5_to_hwpx_bytes_with_options):
/// the same pipeline (decode to Core → map the style store → validate →
/// encode HWPX → patch layout hints), with the shared error and warning
/// model around it. Nothing is read from or written to disk.
///
/// Warnings are passed through, never upgraded to errors. A conversion that
/// drops an unsupported control still returns bytes, with the drop named in
/// `warnings` — the caller decides whether that loss is acceptable.
///
/// # Errors
///
/// [`ConvertOpsError::Hwp5`], classified as `HWP5_DECODE_FAILED` when the
/// container or its records could not be read and `HWP5_CONVERT_FAILED` when
/// the decoded document failed validation. See [`ConvertOpsError::code`] for
/// the full table.
///
/// # Examples
///
/// ```
/// use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options};
/// use hwpforge_foundation::diagnostics::OpsCode;
///
/// let options = ConvertHwp5Options::default().with_carry_layout_cache(true);
/// let error = convert_hwp5(b"PK\x03\x04 not an hwp5", &options)
///     .expect_err("a ZIP is not an OLE2 container");
/// assert_eq!(error.code(), OpsCode::Hwp5DecodeFailed);
/// ```
pub fn convert_hwp5(
    hwp5: &[u8],
    opts: &ConvertHwp5Options,
) -> Result<ConvertHwp5Output, ConvertOpsError> {
    let options = ConvertOptions::default().with_carry_layout_cache(opts.carry_layout_cache);
    let (bytes, warnings) =
        hwp5_to_hwpx_bytes_with_options(hwp5, options).map_err(ConvertOpsError::Hwp5)?;
    Ok(ConvertHwp5Output {
        bytes,
        warnings: warnings.into_iter().map(ConvertOpsWarning::Convert).collect(),
    })
}
