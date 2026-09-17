//! Markdown import: `decode_md`, `convert_md`.
//!
//! # The two entry points
//!
//! [`decode_md`] is the pure first step: Markdown in, a Core document and
//! an **asset plan** out — the list of images the document references and
//! where each one would have to come from. Nothing is read.
//!
//! [`convert_md`] is the convenience path that joins the three steps of the
//! asset contract and ends with HWPX bytes:
//!
//! 1. [`decode_md`] — pure.
//! 2. [`super::fs::resolve_files_from_dir`] — the **only** file I/O in the
//!    operation layer, confined to `base_dir`.
//! 3. `hwpforge_smithy_md::finish_assets` — pure.
//!
//! A caller whose images do not live on a filesystem (in memory, in an
//! object store, on the far side of an FFI boundary) replaces step 2 with
//! its own and keeps the rest.

use std::path::Path;

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::document::Document;
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxEncoder, HwpxRegistryBridge};
use hwpforge_smithy_md::{
    collect_asset_plan, finish_assets, warnings_from, AssetOutcome, AssetPlanEntry, MdDecoder,
};
use serde::{Deserialize, Serialize};

use super::{OpsError, OpsWarning};

/// The only preset `convert_md` accepts today.
///
/// The Markdown decoder resolves the built-in `default` template, and the
/// style store is then built from the registry that template produced. No
/// second template is wired up, so accepting another preset name would
/// return byte-identical output under a different label — fake support.
/// The CLI has the same single-preset gate.
const DEFAULT_PRESET: &str = "default";

/// Options for [`decode_md`] and [`convert_md`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConvertMdOptions {
    /// Style preset name. Only `"default"` exists today; see
    /// [`DEFAULT_PRESET`](self).
    pub preset: String,
}

impl Default for ConvertMdOptions {
    fn default() -> Self {
        Self { preset: DEFAULT_PRESET.to_owned() }
    }
}

impl ConvertMdOptions {
    /// Sets the style preset.
    #[must_use]
    pub fn with_preset(mut self, preset: impl Into<String>) -> Self {
        self.preset = preset.into();
        self
    }

    /// Rejects a preset this operation cannot honour.
    fn check_preset(&self) -> Result<(), OpsError> {
        if self.preset == DEFAULT_PRESET {
            return Ok(());
        }
        Err(OpsError::PresetNotFound { name: self.preset.clone() })
    }
}

/// What [`decode_md`] returns: the document, the styles it was decoded
/// against, and the assets it still needs.
///
/// `document` and `plan` belong together — the plan addresses image runs by
/// **document-order visit number**, so changing the document's image runs
/// between [`decode_md`] and `finish_assets` invalidates the plan.
/// `finish_assets` re-collects the plan and compares it in full rather than
/// trusting the caller.
#[derive(Debug)]
#[non_exhaustive]
pub struct MdDecoded {
    /// The decoded Core document, still in `Draft` state.
    pub document: Document,
    /// The style registry the template resolved, needed to build the HWPX
    /// style store.
    pub style_registry: StyleRegistry,
    /// What each image run needs, in document order.
    pub plan: Vec<AssetPlanEntry>,
    /// Diagnostics raised while decoding.
    ///
    /// The Markdown decoder reports problems as errors rather than
    /// warnings today, so this is empty; it is the channel a future
    /// decoder warning would arrive on, and it keeps the field set the same
    /// as every other operation output.
    pub warnings: Vec<OpsWarning>,
}

/// What [`convert_md`] returns: the HWPX package and what became of each
/// referenced image.
#[derive(Debug)]
#[non_exhaustive]
pub struct ConvertOutput {
    /// The generated HWPX package.
    pub bytes: Vec<u8>,
    /// One entry per planned image, in document order: embedded, dropped,
    /// or skipped because it was remote.
    pub assets: Vec<AssetOutcome>,
    /// Asset exclusions and encode diagnostics.
    pub warnings: Vec<OpsWarning>,
}

impl ConvertOutput {
    /// The serialisable wire payload; the bytes travel beside it.
    #[must_use]
    pub fn meta(&self) -> ConvertMeta {
        ConvertMeta {
            assets: self.assets.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// The `convert_md` wire payload: `{ "assets": [ … ], "warnings": [ … ] }`.
///
/// # Serde
///
/// `Serialize` and `Deserialize` only. [`AssetOutcome`] is smithy-md's
/// type and carries no `JsonSchema` derive, and re-modelling it here to
/// gain one would fork a wire type another crate owns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConvertMeta {
    /// One entry per planned image, in document order.
    pub assets: Vec<AssetOutcome>,
    /// Asset exclusions and encode diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Decodes Markdown into a Core document and the asset plan it implies.
///
/// Pure — no file is opened, and a `data:` image is left inline for
/// `finish_assets` to decode. The preset is checked here so that a typo
/// costs nothing, the same order the CLI uses.
///
/// # Errors
///
/// - [`OpsError::PresetNotFound`] — the preset is not `"default"`.
/// - [`OpsError::MdDecode`] (`MD_DECODE_FAILED`) — the Markdown, its
///   frontmatter or its note definitions could not be decoded;
///   (`INPUT_TOO_LARGE`) — the input exceeds the decoder's size limit.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "ops-md")] {
/// use hwpforge::ops::convert::{decode_md, ConvertMdOptions};
///
/// let decoded = decode_md("# 제목\n\n본문", &ConvertMdOptions::default())?;
/// assert!(decoded.plan.is_empty(), "no images referenced");
/// # }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn decode_md(markdown: &str, opts: &ConvertMdOptions) -> Result<MdDecoded, OpsError> {
    opts.check_preset()?;

    let decoded = MdDecoder::decode_with_default(markdown).map_err(OpsError::md_decode)?;
    let plan = collect_asset_plan(&decoded.document);

    Ok(MdDecoded {
        document: decoded.document,
        style_registry: decoded.style_registry,
        plan,
        warnings: Vec::new(),
    })
}

/// Converts Markdown into an HWPX package, resolving images from
/// `base_dir`.
///
/// This is the one operation that may reach the filesystem, and only to
/// hand `base_dir` to [`super::fs::resolve_files_from_dir`]. With
/// `base_dir = None` every relative image reference is excluded (and
/// reported); inline `data:` images still embed, because their bytes are
/// already in the Markdown.
///
/// # Warnings, not silence
///
/// An image that could not be embedded is **dropped from the document** so
/// that no dangling binary reference reaches the package, and the exclusion
/// is reported twice over: as a typed [`AssetOutcome`] in `assets`, and as
/// a human-readable warning in `warnings`. Generation is not fail-closed on
/// semantic-loss encode warnings the way a regenerating *edit* is — a
/// freshly generated document has no original meaning to preserve — so
/// those arrive in `warnings` too.
///
/// # Errors
///
/// - [`OpsError::PresetNotFound`] — the preset is not `"default"`.
/// - [`OpsError::MdDecode`] (`MD_DECODE_FAILED`) — the Markdown could not
///   be decoded; (`ASSET_PLAN_MISMATCH`, `ASSET_IDENTITY_CONFLICT`) — the
///   resolved assets do not line up with the plan, which for this function
///   would mean a bug in the asset pipeline rather than bad input.
/// - [`OpsError::StyleStore`] (`STYLE_STORE_FAILED`) — the style store could
///   not be built from the template's registry.
/// - [`OpsError::StyleRebind`] (`STYLE_REBIND_FAILED`) — the document's style
///   references could not be rebound to that store.
/// - [`OpsError::Encode`] (`ENCODE_FAILED`) — writing the HWPX package
///   failed.
/// - [`OpsError::Core`] (`VALIDATION_FAILED`) — the rebound document does
///   not validate.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::convert::{convert_md, ConvertMdOptions};
///
/// let markdown = std::fs::read_to_string("report.md")?;
/// let out = convert_md(&markdown, Some(std::path::Path::new("./")), &ConvertMdOptions::default())?;
/// std::fs::write("report.hwpx", &out.bytes)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn convert_md(
    markdown: &str,
    base_dir: Option<&Path>,
    opts: &ConvertMdOptions,
) -> Result<ConvertOutput, OpsError> {
    let MdDecoded { document, style_registry, plan, mut warnings } = decode_md(markdown, opts)?;

    let provided = super::fs::resolve_files_from_dir(&plan, base_dir);
    let finished = finish_assets(document, &plan, provided).map_err(OpsError::md_decode)?;
    // The warnings are *derived* from the outcomes by smithy-md so that the
    // two lists cannot diverge; deriving a second set here would report
    // every excluded image twice.
    warnings.extend(
        warnings_from(&plan, &finished.outcomes)
            .map_err(OpsError::md_decode)?
            .into_iter()
            .map(OpsWarning::Md),
    );

    let bridge =
        HwpxRegistryBridge::from_registry(&style_registry).map_err(OpsError::style_store)?;
    let rebound =
        bridge.rebind_draft_document(finished.document).map_err(OpsError::style_rebind)?;
    let validated = rebound.validate()?;

    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        bridge.style_store(),
        &finished.image_store,
        EncodeOptions::default(),
    )
    .map_err(OpsError::encode)?;
    warnings.extend(outcome.warnings.into_iter().map(OpsWarning::Encode));

    Ok(ConvertOutput { bytes: outcome.bytes, assets: finished.outcomes, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_preset_is_the_one_the_decoder_resolves() {
        assert_eq!(ConvertMdOptions::default().preset, DEFAULT_PRESET);
        assert!(ConvertMdOptions::default().check_preset().is_ok());
    }

    #[test]
    fn any_other_preset_is_refused_rather_than_silently_ignored() {
        // `modern` is a real *restyle* preset, which makes it the most
        // likely thing a caller passes here by mistake.
        let err = ConvertMdOptions::default().with_preset("modern").check_preset().unwrap_err();

        assert_eq!(err.code().as_str(), "PRESET_NOT_FOUND", "{err}");
        assert!(err.to_string().contains("modern"), "{err}");
    }

    #[test]
    fn an_empty_preset_is_not_treated_as_unset() {
        assert!(ConvertMdOptions::default().with_preset("").check_preset().is_err());
    }
}
