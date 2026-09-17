//! PDF export as an operation: [`to_pdf`].

use std::path::PathBuf;

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::{
    render_document, PartialCachePolicy, PdfInput, PdfOptions, RenderFailureMode,
};
use serde::{Deserialize, Serialize};

use super::{ConvertOpsError, ConvertOpsWarning};
use crate::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};

/// OLE2/CFB magic — the HWP5 container.
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Which container the input bytes are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceFormat {
    Hwp5,
    Hwpx,
}

/// Detects the container by **content**, never by extension.
///
/// This repo's own corpus study found 79 HWP5 binaries shipped with a
/// `.hwpx` extension, so the name is not evidence. ZIP is only a candidate
/// here — the HWPX decoder verifies the `mimetype` entry itself.
fn detect_format(bytes: &[u8]) -> Option<SourceFormat> {
    if bytes.starts_with(&CFB_MAGIC) {
        return Some(SourceFormat::Hwp5);
    }
    if bytes.starts_with(b"PK") {
        return Some(SourceFormat::Hwpx);
    }
    None
}

/// Options for [`to_pdf`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ToPdfOptions {
    /// Directories the renderer searches for font files. Empty by default.
    ///
    /// These are **configuration handed to the renderer**, not I/O this layer
    /// performs: smithy-pdf's font resolver opens them while building its
    /// face table. `to_pdf` itself opens no path. A document whose fonts are
    /// found in none of these directories fails with
    /// [`PdfErrorCode::FontUnresolved`](hwpforge_smithy_pdf::PdfErrorCode::FontUnresolved) —
    /// the renderer never substitutes a different typeface, because a
    /// silently swapped font changes the layout it is replaying.
    pub font_dirs: Vec<PathBuf>,

    /// Where the renderer may look **in addition to** `font_dirs`. Default
    /// [`FontDiscovery::ExplicitOnly`], which keeps rendering deterministic
    /// across machines.
    ///
    /// This is the typed enum, not a string. Parsing `"explicit" | "hancom" |
    /// "platform"` belongs to whichever frontend accepts text, which is why
    /// `INVALID_DISCOVERY` is raised there and never here; a frontend that
    /// wants to report it through this crate's error type uses
    /// [`ConvertOpsError::Rejected`].
    pub discovery: FontDiscovery,

    /// Degrade instead of failing when a font face or an image cannot be
    /// honoured. Default `false`.
    ///
    /// Off, a missing bold face or an undecodable image is an error. On, the
    /// renderer falls back to the regular face or omits the image and records
    /// a `"render"` warning. Signal contradictions
    /// (`FontFaceAmbiguous`, `ImageAssetConflict`) stay fatal in both modes,
    /// and this is a degradation policy, never a font-licence bypass.
    pub degraded: bool,

    /// Reject a document that has any paragraph without a layout cache,
    /// instead of skipping those paragraphs. Default `false`.
    ///
    /// Off, cacheless paragraphs are skipped with a `PARAGRAPH_SKIPPED`
    /// warning and the PDF has fewer pages than the Hancom original. On, the
    /// first such paragraph fails the render with cause
    /// `MISSING_LAYOUT_CACHE` — use it when a short PDF is worse than none.
    pub partial_cache_reject: bool,
}

impl ToPdfOptions {
    /// Sets the directories the renderer searches for fonts.
    #[must_use]
    pub fn with_font_dirs(mut self, font_dirs: Vec<PathBuf>) -> Self {
        self.font_dirs = font_dirs;
        self
    }

    /// Sets where the renderer may look beyond `font_dirs`.
    #[must_use]
    pub const fn with_discovery(mut self, discovery: FontDiscovery) -> Self {
        self.discovery = discovery;
        self
    }

    /// Sets whether unhonourable fonts and images degrade instead of failing.
    #[must_use]
    pub const fn with_degraded(mut self, degraded: bool) -> Self {
        self.degraded = degraded;
        self
    }

    /// Sets whether a cacheless paragraph rejects the whole render.
    #[must_use]
    pub const fn with_partial_cache_reject(mut self, reject: bool) -> Self {
        self.partial_cache_reject = reject;
        self
    }

    /// Builds the renderer's own options.
    fn to_pdf_options(&self) -> PdfOptions {
        let mut options = PdfOptions::default();
        options.font_dirs = self.font_dirs.clone();
        options.discovery = self.discovery;
        options.failure_mode =
            if self.degraded { RenderFailureMode::Degraded } else { RenderFailureMode::Fatal };
        options.partial_cache = if self.partial_cache_reject {
            PartialCachePolicy::Reject
        } else {
            PartialCachePolicy::WarnAndSkip
        };
        options
    }
}

/// What [`to_pdf`] returns: the PDF and how much of the document reached it.
#[derive(Debug)]
#[non_exhaustive]
pub struct ToPdfOutput {
    /// The rendered PDF.
    pub bytes: Vec<u8>,
    /// Pages **actually emitted**, counted by the renderer rather than by
    /// re-parsing the bytes.
    ///
    /// Not the source document's page count: under the default partial-cache
    /// policy, skipped paragraphs can make this smaller than what Hancom
    /// shows, and each skip appears in `warnings`.
    pub pages: usize,
    /// Convert, decode and render diagnostics, in pipeline order. Read
    /// [`ConvertOpsWarning::stage`] to tell them apart.
    pub warnings: Vec<ConvertOpsWarning>,
}

impl ToPdfOutput {
    /// The serialisable wire payload; the bytes travel beside it.
    #[must_use]
    pub fn meta(&self) -> PdfMeta {
        PdfMeta {
            pages: self.pages,
            warnings: self.warnings.iter().map(ConvertOpsWarning::info).collect(),
        }
    }
}

/// The `to_pdf` wire payload: `{ "pages": N, "warnings": [ … ] }`.
///
/// # Serde
///
/// `Serialize` and `Deserialize`, plus `JsonSchema` under the `schemars`
/// feature — see [`Hwp5Meta`](super::Hwp5Meta).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PdfMeta {
    /// Pages emitted into the PDF.
    pub pages: usize,
    /// One entry per warning, in pipeline order.
    pub warnings: Vec<WarningInfo>,
}

/// Renders HWP5 or HWPX bytes to PDF.
///
/// The format is detected from the content: an OLE2 header means HWP5 and is
/// converted to HWPX first, a ZIP header means HWPX, and anything else is
/// [`ConvertOpsError::UnrecognizedFormat`]. Then the package is decoded, the
/// document validated, and the renderer replays its layout cache.
///
/// The HWP5 leg always converts with `carry_layout_cache = true`, whatever
/// [`ConvertHwp5Options`](super::ConvertHwp5Options) defaults to: the cached
/// line segments **are** the render material, and without them every
/// paragraph would be cacheless. That is why the flag is not exposed on
/// [`ToPdfOptions`].
///
/// No file is read or written. See [`ToPdfOptions::font_dirs`] for why the
/// font directories are not an exception.
///
/// # Errors
///
/// - [`ConvertOpsError::UnrecognizedFormat`] — the bytes are neither container.
/// - [`ConvertOpsError::Hwp5`] — the HWP5 leg failed.
/// - [`ConvertOpsError::Decode`] — the HWPX package could not be read.
/// - [`ConvertOpsError::Core`] — the decoded document failed validation.
/// - [`ConvertOpsError::Pdf`] — the render failed;
///   [`ConvertOpsError::cause`] carries the renderer's own code.
///
/// # Examples
///
/// ```
/// use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
/// use hwpforge_foundation::diagnostics::OpsCode;
///
/// let error = to_pdf(b"%PDF-1.7 already a pdf", &ToPdfOptions::default())
///     .expect_err("neither an OLE2 nor a ZIP container");
/// assert_eq!(error.code(), OpsCode::UnrecognizedFormat);
/// ```
pub fn to_pdf(data: &[u8], opts: &ToPdfOptions) -> Result<ToPdfOutput, ConvertOpsError> {
    let format = detect_format(data).ok_or(ConvertOpsError::UnrecognizedFormat)?;

    let mut warnings: Vec<ConvertOpsWarning> = Vec::new();
    let converted;
    let hwpx_bytes: &[u8] = match format {
        SourceFormat::Hwp5 => {
            let (bytes, convert_warnings) = hwp5_to_hwpx_bytes_with_options(
                data,
                ConvertOptions::default().with_carry_layout_cache(true),
            )
            .map_err(ConvertOpsError::Hwp5)?;
            warnings.extend(convert_warnings.into_iter().map(ConvertOpsWarning::Convert));
            converted = bytes;
            &converted
        }
        SourceFormat::Hwpx => data,
    };

    let decoded = HwpxDecoder::decode(hwpx_bytes).map_err(ConvertOpsError::Decode)?;
    warnings.extend(decoded.warnings.iter().cloned().map(ConvertOpsWarning::Decode));

    let validated = decoded.document.validate().map_err(ConvertOpsError::Core)?;

    // The bridge is the only plumbing that carries image bytes to the
    // renderer — a bare style store reports `image_data = None`.
    let styles = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let rendered = render_document(
        &PdfInput { document: &validated, styles: &styles },
        &opts.to_pdf_options(),
    )
    .map_err(ConvertOpsError::Pdf)?;
    warnings.extend(rendered.warnings.into_iter().map(ConvertOpsWarning::Render));

    Ok(ToPdfOutput { bytes: rendered.bytes, pages: rendered.pages, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_is_detected_by_content_not_extension() {
        assert_eq!(detect_format(&CFB_MAGIC), Some(SourceFormat::Hwp5));
        assert_eq!(detect_format(b"PK\x03\x04zipzip"), Some(SourceFormat::Hwpx));
        assert_eq!(detect_format(b"not a container"), None);
        assert_eq!(detect_format(b""), None);
        // A truncated OLE2 header is not an OLE2 header.
        assert_eq!(detect_format(&CFB_MAGIC[..4]), None);
    }

    #[test]
    fn toggles_reach_the_renderer_options() {
        let defaults = ToPdfOptions::default().to_pdf_options();
        assert_eq!(defaults.failure_mode, RenderFailureMode::Fatal);
        assert_eq!(defaults.partial_cache, PartialCachePolicy::WarnAndSkip);
        assert_eq!(defaults.discovery, FontDiscovery::ExplicitOnly);
        assert!(defaults.font_dirs.is_empty());

        let opted = ToPdfOptions::default()
            .with_degraded(true)
            .with_partial_cache_reject(true)
            .with_discovery(FontDiscovery::HancomBundle)
            .with_font_dirs(vec![PathBuf::from("/fonts")])
            .to_pdf_options();
        assert_eq!(opted.failure_mode, RenderFailureMode::Degraded);
        assert_eq!(opted.partial_cache, PartialCachePolicy::Reject);
        assert_eq!(opted.discovery, FontDiscovery::HancomBundle);
        assert_eq!(opted.font_dirs, vec![PathBuf::from("/fonts")]);
    }
}
