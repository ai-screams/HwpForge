//! Shared operation layer for the two **conversion** operations — one
//! implementation of each, called by every frontend (CLI, MCP server, Python
//! bindings).
//!
//! # Why these two live here and not in the umbrella crate
//!
//! `hwpforge` (the umbrella) is published to crates.io, and cargo forbids a
//! published crate from depending on an unpublished one. `convert_hwp5` needs
//! smithy-hwp5 and `to_pdf` needs smithy-pdf; both are `publish = false`, so
//! neither operation can live in the umbrella. This crate already sits above
//! smithy-hwp5 and smithy-hwpx and is itself unpublished, so it is the one
//! place both fit.
//!
//! The reverse edge is deliberately absent: convert does **not** depend on the
//! umbrella. Doing so would point an inner crate at the outer facade and drag
//! blueprint and smithy-md into convert's build graph. The single stable code
//! table ([`OpsCode`]) lives in `hwpforge-foundation`, which every crate
//! already depends on, so sharing codes costs no new edge.
//!
//! # Shape
//!
//! The surface mirrors `hwpforge::ops`: one free function per operation,
//! bytes in, a [`Default`] options struct with consuming `with_*` builders,
//! and `Result<XxxOutput, ConvertOpsError>` out. Output structs are
//! `#[non_exhaustive]` and do **not** derive serde; the serialisable payload
//! is the `*Meta` wrapper each output builds through `meta()`, and warnings
//! become [`WarningInfo`] through [`ConvertOpsWarning::info`].
//!
//! # Purity
//!
//! Neither operation reads or writes a file. [`ToPdfOptions::font_dirs`] is
//! the one thing that looks like I/O and is not: the directories are handed
//! to the renderer as configuration, and the renderer's own font resolver
//! opens them. That is smithy-pdf's pre-existing I/O, unchanged by this
//! layer — no path is opened, created or written here. Callers that must
//! avoid all disk access leave `font_dirs` empty and accept
//! [`PdfErrorCode::FontUnresolved`] for any face the document names.
//!
//! # Errors and codes
//!
//! [`ConvertOpsError`] wraps library errors without normalising them, and
//! [`ConvertOpsError::code`] classifies each as an [`OpsCode`]. The mapping
//! lives here **once**; the CLI and the Python bindings call `code()` rather
//! than keeping tables of their own.
//!
//! Upstream enums are all `#[non_exhaustive]`, so classification goes through
//! the defining crate's own stable method first ([`Hwp5Error::code`],
//! [`PdfError::code`]) and only what is left falls through to
//! [`OpsCode::UpstreamUnmapped`]. Enums defined in *this* crate
//! ([`ConvertOpsError`], [`ConvertOpsWarning`], [`ConvertWarning`]) are
//! matched without a wildcard so the compiler catches a new variant.
//!
//! # Warnings keep their stage
//!
//! `to_pdf` is a three-stage pipeline and a warning means different things at
//! each stage, so [`ConvertOpsWarning::stage`] reports which one raised it:
//! `"convert"` (HWP5 → HWPX), `"decode"` (reading the HWPX package) or
//! `"render"` (drawing the PDF). Flattening them would hide HWP5 conversion
//! loss behind a green render. Warnings arrive in pipeline order: every
//! convert warning, then every decode warning, then every render warning.

mod hwp5;
mod pdf;

use hwpforge_core::CoreError;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwp5::{Hwp5Error, Hwp5ErrorCode, Hwp5Warning};
use hwpforge_smithy_hwpx::{DecodeWarning, EncodeWarning, HwpxError};
use hwpforge_smithy_pdf::{PdfError, PdfErrorCode, PdfWarning};

use crate::ConvertWarning;

pub use hwp5::{convert_hwp5, ConvertHwp5Options, ConvertHwp5Output, Hwp5Meta};
pub use pdf::{to_pdf, PdfMeta, ToPdfOptions, ToPdfOutput};

/// The code an unrecognised warning variant reports, as the CLI spells it.
const OTHER: &str = "OTHER";

// ── errors ──────────────────────────────────────────────────────

/// Everything a conversion operation can fail with.
///
/// Library errors are wrapped, not normalised: each keeps its own message and
/// source chain, and [`ConvertOpsError::code`] adds the stable classification
/// that frontends report.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ConvertOpsError {
    /// HWP5 read or HWP5 → HWPX conversion failure.
    ///
    /// Classified per variant by [`Hwp5Error::code`] — see
    /// [`ConvertOpsError::code`] for the table and its one known imprecision.
    #[error(transparent)]
    Hwp5(Hwp5Error),

    /// HWPX **decode**-stage failure: reading the package, parsing its XML or
    /// projecting it to Core.
    ///
    /// Encoding has no arm because neither operation encodes HWPX directly:
    /// `convert_hwp5` encodes inside [`crate::hwp5_to_hwpx_bytes_with_options`],
    /// which reports through [`Hwp5Error`], and `to_pdf` only ever reads.
    #[error(transparent)]
    Decode(HwpxError),

    /// Core document validation failed after a successful decode.
    #[error(transparent)]
    Core(CoreError),

    /// PDF rendering failed.
    ///
    /// The renderer's own classification stays reachable through
    /// [`ConvertOpsError::cause`] so a frontend can keep printing the
    /// detailed cause beside the top-level `PDF_RENDER_FAILED`.
    #[error(transparent)]
    Pdf(PdfError),

    /// The input bytes are neither an OLE2 (HWP5) nor a ZIP (HWPX) container.
    #[error("input is neither an OLE2 (HWP5) nor a ZIP (HWPX) container")]
    UnrecognizedFormat,

    /// An argument rejection that reports its own stable code.
    ///
    /// Nothing inside this crate produces it. It exists so a frontend that
    /// validates an argument before the library sees it can report the
    /// rejection through the same error type, with its dedicated code and its
    /// own wording — the same role [`Rejected`](Self::Rejected) plays in
    /// `hwpforge::ops`. `INVALID_DISCOVERY` is the motivating case: parsing
    /// `"explicit" | "hancom" | "platform"` is the frontend's job, because
    /// [`ToPdfOptions::discovery`] is already the typed enum by the time ops
    /// sees it.
    #[error("invalid input: {reason}")]
    Rejected {
        /// The stable code this rejection reports.
        code: OpsCode,
        /// What made the arguments unusable.
        reason: String,
    },
}

impl ConvertOpsError {
    /// The stable code a frontend reports for this failure.
    ///
    /// # The HWP5 table
    ///
    /// [`Hwp5Error`] is `#[non_exhaustive]`, so classification goes through
    /// its own [`Hwp5Error::code`] and every known code is listed explicitly;
    /// a future variant lands on [`OpsCode::UpstreamUnmapped`] instead of
    /// being silently folded into a neighbour.
    ///
    /// | [`Hwp5ErrorCode`] | [`OpsCode`] | why |
    /// | -- | -- | -- |
    /// | `NotHwp5`, `Cfb`, `MissingStream`, `RecordParse`, `UnsupportedVersion`, `PasswordProtected`, `Encoding`, `Io`, `Foundation` | `Hwp5DecodeFailed` | the container, its records or a primitive built from them could not be read |
    /// | `Core` | `Hwp5ConvertFailed` | produced at exactly one place — the `validate()` of the decoded document in [`crate::hwp5_to_hwpx_bytes_with_options`] — so the bytes were read and the conversion is what failed |
    ///
    /// **Known imprecision.** [`crate::hwp5_to_hwpx_bytes_with_options`]
    /// re-labels an HWPX *encode* failure as [`Hwp5Error::Cfb`], and the
    /// layout-hint patch pass reports through `Cfb` and `MissingStream` too.
    /// Those failures therefore report `HWP5_DECODE_FAILED` although they
    /// happen after decoding. The variant is the only stable signal this
    /// layer has, and inventing a distinction from the error's message text
    /// would be guesswork; separating them properly needs an error variant
    /// that smithy-hwp5 does not have today.
    ///
    /// # The rest
    ///
    /// [`Decode`](Self::Decode) is `DECODE_FAILED`, [`Core`](Self::Core) is
    /// `VALIDATION_FAILED`, [`Pdf`](Self::Pdf) is `PDF_RENDER_FAILED` (with
    /// the renderer's own code available through
    /// [`cause`](Self::cause)), [`UnrecognizedFormat`](Self::UnrecognizedFormat)
    /// is `UNRECOGNIZED_FORMAT`, and [`Rejected`](Self::Rejected) carries its
    /// own.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::{convert_hwp5, ConvertHwp5Options};
    /// use hwpforge_foundation::diagnostics::OpsCode;
    ///
    /// let error = convert_hwp5(b"not a document", &ConvertHwp5Options::default())
    ///     .expect_err("not an HWP5 container");
    /// assert_eq!(error.code(), OpsCode::Hwp5DecodeFailed);
    /// assert_eq!(error.code().as_str(), "HWP5_DECODE_FAILED");
    /// ```
    #[must_use]
    pub fn code(&self) -> OpsCode {
        match self {
            Self::Hwp5(e) => hwp5_code(e),
            Self::Decode(_) => OpsCode::DecodeFailed,
            Self::Core(_) => OpsCode::ValidationFailed,
            Self::Pdf(_) => OpsCode::PdfRenderFailed,
            Self::UnrecognizedFormat => OpsCode::UnrecognizedFormat,
            Self::Rejected { code, .. } => *code,
        }
    }

    /// The renderer's own classification, for a PDF render failure only.
    ///
    /// `PDF_RENDER_FAILED` is a single top-level code by design, but it
    /// covers everything from a missing layout cache to a licence-restricted
    /// font. The CLI prints the detail as a nested `cause` block and
    /// corpus tallies depend on the distinction, so the finer code stays
    /// reachable instead of being collapsed here.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
    ///
    /// let error = to_pdf(b"neither container", &ToPdfOptions::default())
    ///     .expect_err("unrecognised bytes");
    /// assert!(error.cause().is_none(), "only a render failure has a cause");
    /// ```
    #[must_use]
    pub fn cause(&self) -> Option<PdfErrorCode> {
        match self {
            Self::Pdf(e) => Some(e.code()),
            Self::Hwp5(_)
            | Self::Decode(_)
            | Self::Core(_)
            | Self::UnrecognizedFormat
            | Self::Rejected { .. } => None,
        }
    }

    /// A static recovery suggestion for this failure class, if there is one.
    ///
    /// The hints reproduce what the CLI prints today. Hints that quote a
    /// value are not static, so they stay with the frontend that formats them.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::{to_pdf, ToPdfOptions};
    ///
    /// let error = to_pdf(b"neither container", &ToPdfOptions::default())
    ///     .expect_err("unrecognised bytes");
    /// assert!(error.hint().is_some());
    /// ```
    #[must_use]
    pub fn hint(&self) -> Option<&'static str> {
        hint_for(self.code())
    }
}

fn hint_for(code: OpsCode) -> Option<&'static str> {
    Some(match code {
        OpsCode::Hwp5DecodeFailed => "Check that the file is a valid HWP5 document",
        OpsCode::Hwp5ConvertFailed => {
            "Check that the source is a supported HWP5 document"
        }
        OpsCode::DecodeFailed => "Check that the file is a valid HWPX document",
        OpsCode::UnrecognizedFormat => {
            "the format is detected by content, not by extension — pass HWP5 (OLE2) or HWPX (ZIP) bytes"
        }
        OpsCode::PdfRenderFailed => {
            "cacheless documents need a Hancom re-save; font errors may need font_dirs/discovery or degraded"
        }
        OpsCode::UpstreamUnmapped => {
            "이 버전의 hwpforge 가 모르는 상류 오류입니다 — 업그레이드하거나 이슈로 보고하세요"
        }
        _ => return None,
    })
}

/// Classifies an HWP5 failure. See [`ConvertOpsError::code`] for the table
/// and the `Cfb` imprecision it documents.
fn hwp5_code(error: &Hwp5Error) -> OpsCode {
    match error.code() {
        Hwp5ErrorCode::NotHwp5
        | Hwp5ErrorCode::Cfb
        | Hwp5ErrorCode::MissingStream
        | Hwp5ErrorCode::RecordParse
        | Hwp5ErrorCode::UnsupportedVersion
        | Hwp5ErrorCode::PasswordProtected
        | Hwp5ErrorCode::Encoding
        | Hwp5ErrorCode::Io
        | Hwp5ErrorCode::Foundation => OpsCode::Hwp5DecodeFailed,
        Hwp5ErrorCode::Core => OpsCode::Hwp5ConvertFailed,
        _ => OpsCode::UpstreamUnmapped,
    }
}

// ── warnings ────────────────────────────────────────────────────

/// One non-fatal diagnostic from a conversion operation, tagged with the
/// pipeline stage that raised it.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum ConvertOpsWarning {
    /// Raised while converting HWP5 to HWPX (stage `"convert"`).
    Convert(ConvertWarning),
    /// Raised while decoding the HWPX package (stage `"decode"`).
    Decode(DecodeWarning),
    /// Raised while rendering the PDF (stage `"render"`).
    Render(PdfWarning),
}

impl ConvertOpsWarning {
    /// Which pipeline stage raised this warning: `"convert"`, `"decode"` or
    /// `"render"`.
    ///
    /// The CLI's `to-pdf` has a fourth stage, `"input"`, for its
    /// `EXTENSION_MISMATCH` warning. That one has no counterpart here: ops
    /// receives bytes and never sees a filename, so there is no extension to
    /// disagree with the content. It stays a frontend concern.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::ConvertOpsWarning;
    /// use hwpforge_smithy_pdf::PdfWarning;
    ///
    /// let warning = ConvertOpsWarning::Render(PdfWarning::ParagraphSkipped {
    ///     location: "s0/p1".into(),
    /// });
    /// assert_eq!(warning.stage(), "render");
    /// ```
    #[must_use]
    pub const fn stage(&self) -> &'static str {
        match self {
            Self::Convert(_) => "convert",
            Self::Decode(_) => "decode",
            Self::Render(_) => "render",
        }
    }

    /// The wire shape of this warning: stable code, message, optional hint.
    ///
    /// The codes are exactly the strings the CLI's `to-pdf` warning DTOs
    /// emit, `OTHER` included, so a frontend that filters on them keeps
    /// working. The messages are the CLI's messages with one change:
    /// [`WarningInfo`] has no `location` field, so a warning that carried one
    /// gets it prefixed as `"{location}: {message}"` rather than dropped.
    /// That is the same convention `hwpforge::ops` uses.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_convert::ops::ConvertOpsWarning;
    /// use hwpforge_smithy_pdf::PdfWarning;
    ///
    /// let warning = ConvertOpsWarning::Render(PdfWarning::ParagraphSkipped {
    ///     location: "s0/p1".into(),
    /// });
    /// let info = warning.info();
    /// assert_eq!(info.code, "PARAGRAPH_SKIPPED");
    /// assert_eq!(info.message, "s0/p1: paragraph without layout cache skipped");
    /// ```
    #[must_use]
    pub fn info(&self) -> WarningInfo {
        let (code, message, location) = match self {
            Self::Convert(w) => convert_warning_parts(w),
            Self::Decode(w) => decode_warning_parts(w),
            Self::Render(w) => render_warning_parts(w),
        };
        WarningInfo::new(code, locate(message, location))
    }
}

/// Folds a warning's location into its message, because [`WarningInfo`] has
/// no separate field for one.
fn locate(message: String, location: Option<String>) -> String {
    match location {
        Some(location) => format!("{location}: {message}"),
        None => message,
    }
}

/// Reproduces the CLI's `convert_warning_dto` (`commands/to_pdf.rs`).
fn convert_warning_parts(warning: &ConvertWarning) -> (&'static str, String, Option<String>) {
    let hwp5 = match warning {
        ConvertWarning::Hwp5(w) => w,
        // An encode-side cache drop names the paragraph path and the reason;
        // both are worth keeping, so it gets its own arm rather than `OTHER`.
        ConvertWarning::HwpxEncode(EncodeWarning::LayoutCacheDropped { path, reason }) => {
            return ("LAYOUT_CACHE_DROPPED", reason.clone(), Some(path.to_string()));
        }
        ConvertWarning::HwpxEncode(_) => {
            return (OTHER, format!("{warning:?}"), None);
        }
    };
    hwp5_warning_parts(hwp5)
}

fn hwp5_warning_parts(warning: &Hwp5Warning) -> (&'static str, String, Option<String>) {
    match warning {
        Hwp5Warning::UnsupportedTag { tag_id, offset } => (
            "UNSUPPORTED_TAG",
            format!("unsupported record tag 0x{tag_id:02X}"),
            Some(format!("offset {offset}")),
        ),
        Hwp5Warning::SkippedStream { name } => {
            ("SKIPPED_STREAM", format!("stream skipped: {name}"), None)
        }
        Hwp5Warning::DroppedControl { control, reason } => {
            ("DROPPED_CONTROL", format!("{control}: {reason}"), None)
        }
        Hwp5Warning::ProjectionFallback { subject, reason } => {
            ("PROJECTION_FALLBACK", format!("{subject}: {reason}"), None)
        }
        Hwp5Warning::ParserFallback { subject, reason } => {
            ("PARSER_FALLBACK", format!("{subject}: {reason}"), None)
        }
        Hwp5Warning::LayoutCacheDropped { reason } => {
            ("LAYOUT_CACHE_DROPPED", reason.clone(), None)
        }
        other => (OTHER, format!("{other:?}"), None),
    }
}

/// Reproduces the CLI's `decode_warning_dto` (`commands/to_pdf.rs`).
fn decode_warning_parts(warning: &DecodeWarning) -> (&'static str, String, Option<String>) {
    match warning {
        DecodeWarning::UnknownEnumValue { attribute, raw, fallback } => (
            "UNKNOWN_ENUM_VALUE",
            format!("\"{raw}\" unknown — fell back to {fallback}"),
            Some((*attribute).to_string()),
        ),
        other => (OTHER, format!("{other:?}"), None),
    }
}

/// Reproduces the CLI's `render_warning_dto` (`commands/to_pdf.rs`).
fn render_warning_parts(warning: &PdfWarning) -> (&'static str, String, Option<String>) {
    match warning {
        PdfWarning::ParagraphSkipped { location } => (
            "PARAGRAPH_SKIPPED",
            "paragraph without layout cache skipped".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::PageEventLost { location } => (
            "PAGE_EVENT_LOST",
            "page-number restart/hiding control on a cacheless paragraph — event lost".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::FontStyleFallback { face, requested, location } => (
            "FONT_STYLE_FALLBACK",
            format!("{face:?} has no {requested:?} face — rendered regular"),
            Some(location.clone()),
        ),
        PdfWarning::FontAxisFallback { fonts, location } => (
            "FONT_AXIS_FALLBACK",
            format!("per-language axis fonts {fonts:?} — rendered with hangul axis"),
            Some(location.clone()),
        ),
        PdfWarning::FontEmbedPreviewPrint { face, path, .. } => (
            "FONT_EMBED_PREVIEW_PRINT",
            format!("{face:?} ({}) is Preview & Print licensed", path.display()),
            None,
        ),
        PdfWarning::AlignmentApproximated { location } => (
            "ALIGNMENT_APPROXIMATED",
            "distributed alignment approximated".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::NonTextRunDropped { location } => (
            "NON_TEXT_RUN_DROPPED",
            "non-text run (control/image) dropped".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::AnchorMarkerOnLineBoundary { location } => (
            "ANCHOR_MARKER_ON_LINE_BOUNDARY",
            "anchored-image marker sits exactly on a line boundary — \
             anchored to the later line"
                .to_string(),
            Some(location.clone()),
        ),
        PdfWarning::ImageDataMissing { key, location } => (
            "IMAGE_DATA_MISSING",
            format!("image data missing for \"{key}\" — skipped"),
            Some(location.clone()),
        ),
        PdfWarning::UnsupportedImageFormat { key, format, location } => (
            "UNSUPPORTED_IMAGE_FORMAT",
            format!("\"{key}\" is {format} — not renderable, skipped"),
            Some(location.clone()),
        ),
        PdfWarning::ImageDecodeFailed { key, detail, location } => (
            "IMAGE_DECODE_FAILED",
            format!("\"{key}\": {detail} — skipped"),
            Some(location.clone()),
        ),
        PdfWarning::InvalidImageGeometry { key, detail, location } => (
            "INVALID_IMAGE_GEOMETRY",
            format!("\"{key}\": {detail} — skipped"),
            Some(location.clone()),
        ),
        PdfWarning::TablePaginationComputed { location } => (
            "TABLE_PAGINATION_COMPUTED",
            "split-table page boundary computed (cache has no signal)".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::TableDeficitDistributed { location } => (
            "TABLE_DEFICIT_DISTRIBUTED",
            "merged-cell height deficit redistributed".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::UnsupportedTableStyle { location, what } => (
            "UNSUPPORTED_TABLE_STYLE",
            format!("unsupported table style dropped: {what}"),
            Some(location.clone()),
        ),
        PdfWarning::BandOverflow { kind, location } => (
            "BAND_OVERFLOW",
            format!("{kind} exceeds its band — replayed unclipped (Hancom behavior)"),
            Some(location.clone()),
        ),
        PdfWarning::PageStartsOnFallback { section } => (
            "PAGE_STARTS_ON_FALLBACK",
            "pageStartsOn != BOTH is unmeasured — rendered as BOTH".to_string(),
            Some(format!("s{section}")),
        ),
        PdfWarning::VertAlignFallback { location } => (
            "VERT_ALIGN_FALLBACK",
            "header/footer vertAlign != TOP is unmeasured — rendered as TOP".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::PageNumberSkipped { section, what } => (
            "PAGE_NUMBER_SKIPPED",
            format!("page number skipped — unmeasured {what}"),
            Some(format!("s{section}")),
        ),
        PdfWarning::PageNumberStyleFallback { section } => (
            "PAGE_NUMBER_STYLE_FALLBACK",
            "\"쪽 번호\" CHAR style absent — fell back to default char shape".to_string(),
            Some(format!("s{section}")),
        ),
        PdfWarning::MissingGlyphs { face, count, location } => (
            "MISSING_GLYPHS",
            format!("{face:?} lacks glyphs for {count} character(s) — rendered as tofu"),
            Some(location.clone()),
        ),
        PdfWarning::LineOverflow { location, excess } => (
            "LINE_OVERFLOW",
            format!(
                "line exceeds its cached box by {excess} HWPUNIT (char spacing/scale not carried)"
            ),
            Some(location.clone()),
        ),
        other => (OTHER, format!("{other:?}"), None),
    }
}

#[cfg(test)]
mod tests;
