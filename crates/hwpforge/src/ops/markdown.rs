//! Markdown export: `to_md`.
//!
//! Three renderings of the same document, and the choice is the caller's:
//! `styled` keeps as much presentation as GFM plus inline HTML can carry
//! and extracts the images, `lossy` produces plain readable Markdown and
//! reports what that costs, `lossless` produces Markdown the decoder can
//! turn back into the same document.

use std::collections::BTreeMap;

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::MdEncoder;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

use super::{OpsError, OpsWarning};

/// Which Markdown rendering [`to_md`] should produce.
///
/// A plain enum, not `#[non_exhaustive]`: a new mode is a new rendering
/// every caller in this workspace has to decide about.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MdMode {
    /// Style-aware conversion; the only mode that extracts images.
    #[default]
    Styled,
    /// Readable Markdown without style information. Reports what the plain
    /// GFM grid cannot express (merged table cells).
    Lossy,
    /// Round-trip-safe Markdown with YAML frontmatter.
    Lossless,
}

impl MdMode {
    /// The wire spelling the CLI and the FFI use.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Styled => "styled",
            Self::Lossy => "lossy",
            Self::Lossless => "lossless",
        }
    }

    /// Parses a wire spelling, or `None` when no mode has that name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "styled" => Some(Self::Styled),
            "lossy" => Some(Self::Lossy),
            "lossless" => Some(Self::Lossless),
            _ => None,
        }
    }
}

/// Options for [`to_md`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct MdExportOptions {
    /// Which rendering to produce; [`MdMode::Styled`] by default, as in
    /// the CLI.
    pub mode: MdMode,
}

impl MdExportOptions {
    /// Selects the rendering.
    #[must_use]
    pub fn with_mode(mut self, mode: MdMode) -> Self {
        self.mode = mode;
        self
    }

    /// Selects the rendering by its wire spelling.
    ///
    /// # Errors
    ///
    /// [`OpsError::InvalidInput`] when no mode has that name.
    pub fn with_mode_name(self, name: &str) -> Result<Self, OpsError> {
        let mode = MdMode::parse(name).ok_or_else(|| OpsError::InvalidInput {
            reason: format!(
                "unknown markdown mode '{name}' — available modes: styled, lossy, lossless"
            ),
        })?;
        Ok(self.with_mode(mode))
    }
}

/// What [`to_md`] returns: the Markdown, the images it referenced, and the
/// warnings raised along the way.
#[derive(Debug)]
#[non_exhaustive]
pub struct MdExportOutput {
    /// The generated Markdown.
    pub markdown: String,
    /// The rendering that produced it.
    pub mode: MdMode,
    /// Images the Markdown references, keyed by the relative path it uses.
    ///
    /// Only [`MdMode::Styled`] extracts images; the other two modes leave
    /// this empty. A `BTreeMap` (the encoder hands back a `HashMap`) so
    /// that the order is the same on every run.
    pub images: BTreeMap<String, Vec<u8>>,
    /// Decode warnings, plus what the lossy rendering could not express.
    pub warnings: Vec<OpsWarning>,
}

impl MdExportOutput {
    /// The serialisable wire payload; the Markdown travels beside it.
    #[must_use]
    pub fn meta(&self) -> MdExportMeta {
        MdExportMeta {
            mode: self.mode.as_str().to_owned(),
            images: self
                .images
                .iter()
                .map(|(key, bytes)| (key.clone(), ByteBuf::from(bytes.clone())))
                .collect(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// The `to_md` wire payload: `{ "mode": …, "images": { … }, "warnings": [ … ] }`.
///
/// # Why `ByteBuf`
///
/// The image values are bytes, and `serde_bytes::ByteBuf` is what makes a
/// serde-driven bridge emit them as bytes. A plain `Vec<u8>` serialises as
/// a sequence of integers, so `pythonize` would hand Python a `list[int]`
/// where it wants `bytes`.
///
/// # Serde
///
/// `Serialize` and `Deserialize` only: `schemars` has no `JsonSchema`
/// implementation for `ByteBuf`, and this payload is not part of the
/// published exchange schema anyway — [`schema`](fn@super::schema) describes the JSON
/// document types, not the FFI result envelopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MdExportMeta {
    /// The rendering that produced the Markdown.
    pub mode: String,
    /// Images the Markdown references, keyed by the relative path it uses.
    pub images: BTreeMap<String, ByteBuf>,
    /// Decode warnings, plus what the lossy rendering could not express.
    pub warnings: Vec<WarningInfo>,
}

/// Renders an HWPX document as Markdown.
///
/// Decodes, validates, and encodes — nothing is read from or written to
/// disk, so the images come back in memory rather than in a sibling
/// `images/` directory the way the CLI writes them.
///
/// # Warnings
///
/// Decode warnings are always reported. [`MdMode::Lossy`] additionally
/// reports what the plain GFM grid dropped (`TABLE_MERGE_FLATTENED` for a
/// table whose merged cells were flattened); [`MdMode::Styled`] renders
/// those tables as HTML instead and so has nothing to report.
///
/// # Errors
///
/// - [`OpsError::Decode`] (`DECODE_FAILED`) — the bytes are not a decodable
///   HWPX package.
/// - [`OpsError::Core`] (`VALIDATION_FAILED`) — the decoded document does
///   not validate.
/// - [`OpsError::MdEncode`] (`ENCODE_FAILED`) — the Markdown encoder
///   refused the document. Only [`MdMode::Lossy`] and [`MdMode::Lossless`]
///   can fail this way; the styled encoder is infallible.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::markdown::{to_md, MdExportOptions, MdMode};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = to_md(&bytes, &MdExportOptions::default().with_mode(MdMode::Lossless))?;
/// println!("{}", out.markdown);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn to_md(hwpx: &[u8], opts: &MdExportOptions) -> Result<MdExportOutput, OpsError> {
    let decoded = HwpxDecoder::decode(hwpx).map_err(OpsError::decode)?;
    let mut warnings: Vec<OpsWarning> =
        decoded.warnings.iter().cloned().map(OpsWarning::Decode).collect();
    let document = decoded.document.validate()?;

    let (markdown, images) = match opts.mode {
        MdMode::Styled => {
            let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
            let output = MdEncoder::encode_styled(&document, &lookup);
            (output.markdown, output.images.into_iter().collect())
        }
        MdMode::Lossy => {
            let (markdown, lossy) =
                MdEncoder::encode_lossy_with_report(&document).map_err(OpsError::md_encode)?;
            warnings.extend(lossy.into_iter().map(OpsWarning::Md));
            (markdown, BTreeMap::new())
        }
        MdMode::Lossless => {
            (MdEncoder::encode_lossless(&document).map_err(OpsError::md_encode)?, BTreeMap::new())
        }
    };

    Ok(MdExportOutput { markdown, mode: opts.mode, images, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_spellings_round_trip() {
        for mode in [MdMode::Styled, MdMode::Lossy, MdMode::Lossless] {
            assert_eq!(MdMode::parse(mode.as_str()), Some(mode), "{}", mode.as_str());
        }
    }

    #[test]
    fn serde_uses_the_same_spelling_as_as_str() {
        let json = serde_json::to_value(MdMode::Lossless).expect("serialise");
        assert_eq!(json, serde_json::json!("lossless"));
    }

    #[test]
    fn styled_is_the_default_mode() {
        assert_eq!(MdExportOptions::default().mode, MdMode::Styled);
    }

    #[test]
    fn an_unknown_mode_name_is_invalid_input() {
        let err = MdExportOptions::default().with_mode_name("pretty").expect_err("must reject");

        assert_eq!(err.code().as_str(), "INVALID_INPUT", "{err}");
        assert!(err.to_string().contains("lossless"), "{err}");
    }
}
