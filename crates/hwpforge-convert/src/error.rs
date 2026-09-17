//! Staged failure types for the HWP5 → HWPX pipeline.
//!
//! # Why a convert-owned error exists
//!
//! The conversion runs four stages — decode the HWP5 container, validate the
//! decoded Core document, encode it as an HWPX package, replay the captured
//! layout hints onto those bytes — but until now every one of them reported
//! through [`Hwp5Error`]. The last three had no variant of their own, so an
//! HWPX encoder failure travelled as [`Hwp5Error::Cfb`] and a missing entry in
//! the *generated* package as [`Hwp5Error::MissingStream`]. Both classify as
//! `HWP5_DECODE_FAILED`, which says the source file could not be read when in
//! fact it was read fine and the conversion is what broke.
//!
//! [`ConvertError`] names the stage instead of borrowing a neighbour's
//! variant. The legacy `Hwp5Result` entrypoints keep their signatures and
//! reproduce the exact [`Hwp5Error`] values they produced before through
//! [`ConvertError::into_hwp5_error`], so nothing that already consumes them
//! sees a different error.

use hwpforge_core::CoreError;
use hwpforge_smithy_hwp5::Hwp5Error;
use hwpforge_smithy_hwpx::HwpxError;

/// Which stage of the HWP5 → HWPX conversion failed, and with what.
///
/// Returned by [`hwp5_to_hwpx_bytes_with_diagnostics`](crate::hwp5_to_hwpx_bytes_with_diagnostics).
/// The stage is the whole point: only [`Decode`](Self::Decode) means the input
/// file was unreadable; the other three mean it was read and something
/// downstream of it failed.
///
/// # Examples
///
/// ```
/// use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertError, ConvertOptions};
///
/// let error = hwp5_to_hwpx_bytes_with_diagnostics(b"not a document", ConvertOptions::default())
///     .expect_err("not an OLE2 container");
/// assert_eq!(error.stage(), "decode");
/// assert!(matches!(error, ConvertError::Decode(_)));
/// ```
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConvertError {
    /// Reading the HWP5 container, its records or its style store failed.
    ///
    /// This is the only stage that says anything about the *input file*.
    #[error(transparent)]
    Decode(Hwp5Error),

    /// The decoded document failed Core validation.
    ///
    /// The bytes were read; the document they describe is not one this
    /// library is willing to encode.
    #[error("decoded document failed validation: {0}")]
    Validate(#[source] CoreError),

    /// Encoding the validated document as an HWPX package failed.
    #[error("HWPX encoding failed: {0}")]
    Encode(#[source] HwpxError),

    /// Replaying the captured HWP5 layout hints onto the generated HWPX
    /// package failed.
    ///
    /// Everything this stage touches is a package **this crate just
    /// produced** — a failure here is an internal inconsistency, never a
    /// statement about the source file.
    #[error(transparent)]
    LayoutPatch(#[from] LayoutPatchError),
}

impl ConvertError {
    /// The pipeline stage that raised this failure: `"decode"`,
    /// `"validate"`, `"encode"` or `"layout-patch"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};
    ///
    /// let error = hwp5_to_hwpx_bytes_with_diagnostics(b"PK\x03\x04", ConvertOptions::default())
    ///     .expect_err("a ZIP is not an OLE2 container");
    /// assert_eq!(error.stage(), "decode");
    /// ```
    #[must_use]
    pub const fn stage(&self) -> &'static str {
        match self {
            Self::Decode(_) => "decode",
            Self::Validate(_) => "validate",
            Self::Encode(_) => "encode",
            Self::LayoutPatch(_) => "layout-patch",
        }
    }

    /// The [`Hwp5Error`] the legacy entrypoints report for this failure.
    ///
    /// This is the compatibility bridge, not a recommendation: it reproduces
    /// the values [`hwp5_to_hwpx_bytes`](crate::hwp5_to_hwpx_bytes) and its
    /// siblings produced before the stages existed, variant for variant and
    /// message for message.
    ///
    /// | stage | legacy [`Hwp5Error`] |
    /// | -- | -- |
    /// | [`Decode`](Self::Decode) | the decoder's own error, untouched |
    /// | [`Validate`](Self::Validate) | [`Hwp5Error::Core`] |
    /// | [`Encode`](Self::Encode) | [`Hwp5Error::Cfb`], detail `"HWPX encoding failed: {error}"` |
    /// | [`LayoutPatch`](Self::LayoutPatch) | [`Hwp5Error::Cfb`] or [`Hwp5Error::MissingStream`], per [`LayoutPatchError`] |
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ConvertError;
    /// use hwpforge_smithy_hwp5::Hwp5Error;
    ///
    /// let staged = ConvertError::Decode(Hwp5Error::PasswordProtected);
    /// assert!(matches!(staged.into_hwp5_error(), Hwp5Error::PasswordProtected));
    /// ```
    #[must_use]
    pub fn into_hwp5_error(self) -> Hwp5Error {
        match self {
            Self::Decode(error) => error,
            Self::Validate(error) => Hwp5Error::Core(error),
            Self::Encode(error) => {
                Hwp5Error::Cfb { detail: format!("HWPX encoding failed: {error}") }
            }
            Self::LayoutPatch(LayoutPatchError::Package { detail }) => Hwp5Error::Cfb { detail },
            Self::LayoutPatch(LayoutPatchError::MissingEntry { name }) => {
                Hwp5Error::MissingStream { name }
            }
        }
    }
}

/// What the layout-hint replay can fail with.
///
/// Both variants describe the **generated** HWPX package, which is why they
/// are not decoder errors: the source file was already read successfully by
/// the time this stage runs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutPatchError {
    /// The generated package could not be read, rewritten or repacked.
    #[error("layout-hint patch: {detail}")]
    Package {
        /// What went wrong, as the ZIP or XML layer reported it.
        detail: String,
    },

    /// A section entry the captured hints name is absent from the generated
    /// package.
    #[error("layout-hint patch: generated package has no entry '{name}'")]
    MissingEntry {
        /// The entry path that was expected.
        name: String,
    },
}

/// The layout-hint replay's own result type.
pub(crate) type LayoutPatchResult<T> = Result<T, LayoutPatchError>;

#[cfg(test)]
mod tests {
    use hwpforge_core::CoreError;
    use hwpforge_smithy_hwp5::{Hwp5Error, Hwp5ErrorCode};
    use hwpforge_smithy_hwpx::HwpxError;

    use super::{ConvertError, LayoutPatchError};
    use crate::{hwp5_to_hwpx_bytes, hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions};

    fn core_error() -> CoreError {
        CoreError::InvalidStructure { context: "document".into(), reason: "no sections".into() }
    }

    #[test]
    fn every_stage_maps_back_to_the_legacy_hwp5_error_byte_for_byte() {
        // The compatibility lock. Each row is a stage, the `Hwp5Error` the
        // legacy entrypoints produced before the stages existed, and that
        // error's exact message. A staged error that stopped reproducing one
        // of these would change what every existing caller sees.
        let cases: Vec<(ConvertError, Hwp5ErrorCode, &str)> = vec![
            (
                ConvertError::Decode(Hwp5Error::NotHwp5 { detail: "no magic".into() }),
                Hwp5ErrorCode::NotHwp5,
                "Not a valid HWP5 file: no magic",
            ),
            (
                ConvertError::Validate(core_error()),
                Hwp5ErrorCode::Core,
                "Core error: Invalid document structure in document: no sections",
            ),
            (
                ConvertError::Encode(HwpxError::XmlSerialize { detail: "boom".into() }),
                Hwp5ErrorCode::Cfb,
                "OLE2/CFB error: HWPX encoding failed: XML serialization error: boom",
            ),
            (
                ConvertError::LayoutPatch(LayoutPatchError::Package {
                    detail: "open hwpx package: bad zip".into(),
                }),
                Hwp5ErrorCode::Cfb,
                "OLE2/CFB error: open hwpx package: bad zip",
            ),
            (
                ConvertError::LayoutPatch(LayoutPatchError::MissingEntry {
                    name: "Contents/section3.xml".into(),
                }),
                Hwp5ErrorCode::MissingStream,
                "Missing required HWP5 stream: 'Contents/section3.xml'",
            ),
        ];

        for (staged, code, message) in cases {
            let stage = staged.stage();
            let legacy = staged.into_hwp5_error();
            assert_eq!(legacy.code(), code, "{stage}");
            assert_eq!(legacy.to_string(), message, "{stage}");
        }
    }

    #[test]
    fn the_legacy_entrypoint_returns_what_the_staged_one_maps_to() {
        // Same bytes through both doors: the wrapper is the staged call plus
        // `into_hwp5_error`, with nothing else in between.
        let staged =
            hwp5_to_hwpx_bytes_with_diagnostics(b"not a document", ConvertOptions::default())
                .expect_err("not an OLE2 container");
        assert_eq!(staged.stage(), "decode");
        let expected = staged.into_hwp5_error();

        let legacy = hwp5_to_hwpx_bytes(b"not a document").expect_err("not an OLE2 container");
        assert_eq!(legacy.code(), expected.code());
        assert_eq!(legacy.to_string(), expected.to_string());
    }

    #[test]
    fn the_stage_is_the_one_thing_the_staged_error_adds() {
        for (staged, stage) in [
            (ConvertError::Decode(Hwp5Error::PasswordProtected), "decode"),
            (ConvertError::Validate(core_error()), "validate"),
            (ConvertError::Encode(HwpxError::XmlSerialize { detail: "x".into() }), "encode"),
            (
                ConvertError::LayoutPatch(LayoutPatchError::Package { detail: "x".into() }),
                "layout-patch",
            ),
        ] {
            assert_eq!(staged.stage(), stage);
            assert!(!staged.to_string().is_empty(), "{stage} has a blank message");
        }
    }

    #[test]
    fn a_layout_patch_failure_never_claims_the_source_file_was_unreadable() {
        // The finding in one assertion: the staged error says "layout-patch",
        // while the legacy value it maps to is still the old `MissingStream`.
        let staged = ConvertError::LayoutPatch(LayoutPatchError::MissingEntry {
            name: "Contents/section0.xml".into(),
        });
        assert_eq!(staged.stage(), "layout-patch");
        assert!(
            staged.to_string().contains("generated package"),
            "the message names the generated package: {staged}"
        );
        assert!(matches!(staged.into_hwp5_error(), Hwp5Error::MissingStream { .. }));
    }
}
