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
//!
//! Every preset — `"default"` included — inserts one more step between 1
//! and 2: swapping the decoded registry's base font for the preset's own
//! font ([`builtin_presets`]'s declared value, the same one `templates()`
//! advertises). See [`convert_md`]'s own docs for exactly what that swap
//! touches.

use std::path::Path;

use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_core::document::Document;
use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_foundation::FontId;
use hwpforge_smithy_hwpx::{builtin_presets, EncodeOptions, HwpxEncoder, HwpxRegistryBridge};
use hwpforge_smithy_md::{
    collect_asset_plan, finish_assets, warnings_from, AssetOutcome, AssetPlanEntry, MdDecoder,
};
use serde::{Deserialize, Serialize};

use super::{OpsError, OpsWarning};

/// The preset [`ConvertMdOptions::default`] selects when the caller names
/// none.
///
/// [`builtin_presets`] is the contract every frontend's `templates()` call
/// advertises, so [`convert_md`] applies **this** preset's font too, even
/// though the Markdown decoder's own `default` template
/// (`decode_with_default`) may resolve a different base font of its own —
/// see [`convert_md`]'s `# Presets` section for why the two are allowed to
/// disagree and what wins.
const DEFAULT_PRESET: &str = "default";

/// Options for [`decode_md`] and [`convert_md`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConvertMdOptions {
    /// Style preset name — any [`builtin_presets`] entry (`default`,
    /// `modern`, `classic`, `latest`). Every entry's declared font is
    /// applied, `"default"` included; see [`convert_md`].
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

    /// Rejects a preset no built-in style table defines.
    fn check_preset(&self) -> Result<(), OpsError> {
        if builtin_presets().iter().any(|preset| preset.name == self.preset) {
            return Ok(());
        }
        Err(OpsError::PresetNotFound { name: self.preset.clone() })
    }
}

/// Swaps a Markdown-decoded registry's base font — the first font
/// `decode_with_default` resolved — for a preset's font. Any other font
/// entry (a code-block face that never matched the base) is left alone.
/// Called for every preset, `"default"` included; a no-op when the base
/// already equals `new_font`.
///
/// [`HwpxStyleStore::replace_font`](hwpforge_smithy_hwpx::HwpxStyleStore::replace_font)
/// applies the same rule for [`restyle`](super::style::restyle), but on an
/// already-encoded package's style store rather than a pre-encode
/// [`StyleRegistry`]. There is no `StyleRegistry` equivalent today, so this
/// duplicates the *behaviour* privately here instead of adding a public
/// method to `hwpforge-blueprint` — out of this lane's scope; see the W2
/// report.
fn apply_preset_font(registry: &mut StyleRegistry, new_font: &str) -> Result<(), OpsError> {
    let Some(original_base) = registry.fonts.first().map(|font| font.as_str().to_owned()) else {
        return Ok(());
    };
    let new_font_id = FontId::new(new_font)?;
    for font in &mut registry.fonts {
        if font.as_str() == original_base {
            *font = new_font_id.clone();
        }
    }
    for char_shape in &mut registry.char_shapes {
        if char_shape.font == original_base {
            char_shape.font = new_font.to_owned();
        }
    }
    Ok(())
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
    /// Number of sections in the generated document.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections — the same
    /// definition as [`InspectSection::top_level_paragraphs`](super::inspect::InspectSection::top_level_paragraphs),
    /// summed. Measured on the document already in hand before encoding,
    /// so a caller does not have to decode `bytes` again just to report a
    /// size alongside the generated package.
    pub paragraphs: usize,
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
            sections: self.sections,
            paragraphs: self.paragraphs,
            assets: self.assets.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// The `convert_md` wire payload:
/// `{ "sections": …, "paragraphs": …, "assets": [ … ], "warnings": [ … ] }`.
///
/// # Serde
///
/// `Serialize` and `Deserialize` only. [`AssetOutcome`] is smithy-md's
/// type and carries no `JsonSchema` derive, and re-modelling it here to
/// gain one would fork a wire type another crate owns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConvertMeta {
    /// Number of sections in the generated document.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections (top-level only —
    /// see [`ConvertOutput::paragraphs`]).
    pub paragraphs: usize,
    /// One entry per planned image, in document order.
    pub assets: Vec<AssetOutcome>,
    /// Asset exclusions and encode diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Decodes Markdown into a Core document and the asset plan it implies.
///
/// Pure — no file is opened, and a `data:` image is left inline for
/// `finish_assets` to decode. The preset **name** is checked here so that a
/// typo costs nothing, the same order the CLI uses; the font swap itself
/// happens later, in [`convert_md`], once there is a document to swap it
/// into. A caller who only calls `decode_md` gets the registry
/// `decode_with_default` resolved, unswapped.
///
/// # Errors
///
/// - [`OpsError::PresetNotFound`] — no built-in preset has that name.
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
/// # Presets
///
/// Every preset — `"default"` included — swaps every font in the decoded
/// registry that equals its *first* font — the base font
/// `decode_with_default` produced — for the preset's own declared font
/// (`builtin_presets()`'s value, the same one every frontend's
/// `templates()` call advertises). This runs even when `preset` is
/// `"default"`: the Markdown decoder's own `default` template is not
/// guaranteed to already use the font `builtin_presets()` names for
/// `"default"`, and `builtin_presets()` is the contract a caller is told
/// about, not the template file, so it wins (a preset whose font already
/// matches the base is simply a no-op). A specialty font that never
/// matched the base (a code-block face, say) is left alone. Layout, sizes,
/// colors and every other style property are unaffected; this is a
/// face-name substitution, not a different template.
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
/// - [`OpsError::PresetNotFound`] — no built-in preset has that name.
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
    let MdDecoded { document, mut style_registry, plan, mut warnings } = decode_md(markdown, opts)?;

    // Applied for every preset, `"default"` included — `builtin_presets()`
    // is the font contract `templates()` advertises, not the Markdown
    // decoder's own template, and the two are not guaranteed to agree (see
    // `# Presets` above). `decode_md` already proved `opts.preset` names a
    // real preset — `check_preset` ran first — so this lookup cannot miss;
    // the fallback still reports the right error rather than panicking if
    // that invariant is ever broken.
    let preset_font = builtin_presets()
        .into_iter()
        .find(|preset| preset.name == opts.preset)
        .map(|preset| preset.font)
        .ok_or_else(|| OpsError::PresetNotFound { name: opts.preset.clone() })?;
    apply_preset_font(&mut style_registry, &preset_font)?;

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
    let sections = validated.sections().len();
    let paragraphs: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();

    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        bridge.style_store(),
        &finished.image_store,
        EncodeOptions::default(),
    )
    .map_err(OpsError::encode)?;
    warnings.extend(outcome.warnings.into_iter().map(OpsWarning::Encode));

    Ok(ConvertOutput {
        bytes: outcome.bytes,
        sections,
        paragraphs,
        assets: finished.outcomes,
        warnings,
    })
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
    fn every_builtin_preset_name_passes_the_check() {
        // `modern`/`classic`/`latest` are real *restyle* presets, and this
        // operation now applies the same font swap `restyle` does — a
        // caller passing one is not a typo.
        for preset in builtin_presets() {
            assert!(
                ConvertMdOptions::default().with_preset(preset.name.clone()).check_preset().is_ok(),
                "{}",
                preset.name
            );
        }
    }

    #[test]
    fn a_name_no_builtin_preset_has_is_refused_rather_than_silently_ignored() {
        let err =
            ConvertMdOptions::default().with_preset("gov_proposal").check_preset().unwrap_err();

        assert_eq!(err.code().as_str(), "PRESET_NOT_FOUND", "{err}");
        assert!(err.to_string().contains("gov_proposal"), "{err}");
    }

    #[test]
    fn an_empty_preset_is_not_treated_as_unset() {
        assert!(ConvertMdOptions::default().with_preset("").check_preset().is_err());
    }

    #[test]
    fn apply_preset_font_only_swaps_entries_matching_the_original_base() {
        // The default template's `code` style uses D2Coding
        // (`hwpforge-blueprint/templates/default.yaml`), so decoding a
        // document with a code block resolves a registry that already
        // carries a specialty font alongside the base one — real data
        // instead of a hand-built fixture.
        let decoded =
            decode_md("본문입니다.\n\n```rust\nfn main() {}\n```\n", &ConvertMdOptions::default())
                .expect("decode");
        let mut registry = decoded.style_registry;
        let original_base = registry.fonts.first().expect("a base font").as_str().to_owned();
        assert!(
            registry.fonts.iter().any(|f| f.as_str() == "D2Coding"),
            "fixture assumption: the default template's code style resolves D2Coding"
        );
        let font_count = registry.fonts.len();
        let char_shape_count = registry.char_shapes.len();

        apply_preset_font(&mut registry, "맑은 고딕").expect("swap");

        assert_eq!(registry.fonts.len(), font_count, "a swap must not add or drop entries");
        assert_eq!(registry.char_shapes.len(), char_shape_count);
        assert!(!registry.fonts.iter().any(|f| f.as_str() == original_base), "{registry:?}");
        assert!(
            registry.fonts.iter().any(|f| f.as_str() == "D2Coding"),
            "a specialty font must survive: {registry:?}"
        );
        assert!(!registry.char_shapes.iter().any(|cs| cs.font == original_base), "{registry:?}");
        assert!(
            registry.char_shapes.iter().any(|cs| cs.font == "D2Coding"),
            "a specialty char shape must survive: {registry:?}"
        );
    }

    #[test]
    fn apply_preset_font_on_an_empty_registry_is_a_no_op() {
        let mut registry = StyleRegistry::with_fonts(vec![]);

        apply_preset_font(&mut registry, "맑은 고딕").expect("no font to swap, not an error");

        assert!(registry.fonts.is_empty());
    }
}
