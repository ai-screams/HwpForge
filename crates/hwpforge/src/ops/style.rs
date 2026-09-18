//! Presets and validation: `templates`, `restyle`, `validate`.
//!
//! The three operations share one theme — they answer "what styles exist",
//! "apply a different one", and "is this document sound" — and they are the
//! three shapes the operation layer has: a pure query with no input
//! document, a regenerating edit that is fail-closed, and a diagnostic that
//! reports a verdict instead of failing.

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::presets::{builtin_presets, PresetInfo};
use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder};
use serde::Serialize;

use super::{take_bytes_fail_closed, OpsError, OpsWarning};

/// The code [`validate`] reports a failed document check under.
///
/// Validation failure is a *verdict*, not an operation failure, so it
/// travels as a [`WarningInfo`] inside [`ValidateReport::errors`] rather
/// than as an [`OpsError`]. The string matches what the CLI's `convert`
/// prints for the same `Document::validate` failure.
const VALIDATION_FAILED: &str = "VALIDATION_FAILED";

// ── templates ───────────────────────────────────────────────────

/// What [`templates`] returns: the built-in style presets.
///
/// There is no `warnings` field — listing presets reads nothing and
/// therefore cannot diagnose anything.
#[derive(Debug)]
#[non_exhaustive]
pub struct TemplatesOutput {
    /// The built-in presets, in declaration order.
    pub presets: Vec<PresetInfo>,
}

impl TemplatesOutput {
    /// The serialisable wire payload.
    #[must_use]
    pub fn meta(&self) -> TemplateList {
        TemplateList { presets: self.presets.clone() }
    }
}

/// The `templates` wire payload: `{ "presets": [ … ] }`.
///
/// Each entry is [`PresetInfo`] exactly as smithy-hwpx defines it, so the
/// dictionary keys per preset are `name`, `description`, `font` and
/// `page_size` — the same four the CLI's `templates list --json` prints.
///
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct TemplateList {
    /// The built-in presets, in declaration order.
    pub presets: Vec<PresetInfo>,
}

/// Lists the built-in style presets.
///
/// Pure: no input document, no filesystem, and no failure mode — the
/// preset table is compiled in.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "ops-hwpx")] {
/// use hwpforge::ops::style::templates;
///
/// let out = templates();
/// assert!(out.presets.iter().any(|p| p.name == "default"));
/// # }
/// ```
#[must_use]
pub fn templates() -> TemplatesOutput {
    TemplatesOutput { presets: builtin_presets() }
}

// ── restyle ─────────────────────────────────────────────────────

/// Options for [`restyle`].
///
/// [`Default`] leaves `preset` empty, which no preset matches, so a caller
/// that forgets to choose one gets [`OpsError::PresetNotFound`] instead of
/// a silently applied default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RestyleOptions {
    /// Name of the preset whose base font replaces the document's.
    pub preset: String,
}

impl RestyleOptions {
    /// Sets the preset to apply.
    #[must_use]
    pub fn with_preset(mut self, preset: impl Into<String>) -> Self {
        self.preset = preset.into();
        self
    }
}

/// What [`restyle`] returns: the re-encoded document and its warnings.
#[derive(Debug)]
#[non_exhaustive]
pub struct RestyleOutput {
    /// The re-encoded HWPX package.
    pub bytes: Vec<u8>,
    /// The preset that was applied.
    pub preset: String,
    /// Number of sections in the re-encoded document.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections — the same
    /// definition as [`InspectSection::top_level_paragraphs`](super::inspect::InspectSection::top_level_paragraphs),
    /// summed. Measured on the document already in hand before encoding,
    /// so a caller does not have to decode `bytes` again just to report a
    /// size alongside the restyled package.
    pub paragraphs: usize,
    /// Non-semantic encode warnings (semantic loss is an error, not a
    /// warning — see [`restyle`]).
    pub warnings: Vec<OpsWarning>,
}

impl RestyleOutput {
    /// The serialisable wire payload; the bytes travel beside it.
    #[must_use]
    pub fn meta(&self) -> RestyleMeta {
        RestyleMeta {
            preset: self.preset.clone(),
            sections: self.sections,
            paragraphs: self.paragraphs,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// The `restyle` wire payload:
/// `{ "preset": …, "sections": …, "paragraphs": …, "warnings": [ … ] }`.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RestyleMeta {
    /// The preset that was applied.
    pub preset: String,
    /// Number of sections in the re-encoded document.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections (top-level only —
    /// see [`RestyleOutput::paragraphs`]).
    pub paragraphs: usize,
    /// Non-semantic encode warnings.
    pub warnings: Vec<WarningInfo>,
}

/// Applies a style preset's base font to an existing HWPX document.
///
/// The document's own style store is kept and only font face names are
/// swapped, so every char/para shape index the document references stays
/// valid. The first font in the store is the base font by encoder contract
/// (`HwpxStyleStore::push_font` writes it first); a third-party package with
/// a different ordering restyles a different face, which is the behaviour
/// the MCP tool has today.
///
/// # Fail-closed
///
/// `restyle` regenerates the whole package, so it is preserve-first: if the
/// encoder reports a semantic loss (a footnote number head it could not
/// place, a dropped `titleMark`), the output would mean something different
/// from the input and **no bytes are returned**. The classification lives in
/// `EncodeWarning::is_semantic_loss` and reaches here through
/// [`take_bytes_fail_closed`]. Non-semantic warnings (a dropped line-layout
/// cache) are returned in `warnings` as before.
///
/// # Errors
///
/// - [`OpsError::PresetNotFound`] — no built-in preset has that name.
/// - [`OpsError::Decode`] (`DECODE_FAILED`) — the bytes are not a decodable
///   HWPX package.
/// - [`OpsError::Encode`] (`ENCODE_FAILED`) — re-encoding failed.
/// - [`OpsError::NoFonts`] — the document declares no font to rebind.
/// - [`OpsError::Core`] (`VALIDATION_FAILED`) — the decoded document does
///   not validate.
/// - [`OpsError::EncodeSemanticLoss`] — see **Fail-closed** above.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::style::{restyle, RestyleOptions};
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = restyle(&bytes, &RestyleOptions::default().with_preset("modern"))?;
/// std::fs::write("restyled.hwpx", &out.bytes)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn restyle(hwpx: &[u8], opts: &RestyleOptions) -> Result<RestyleOutput, OpsError> {
    // Preset lookup first: a typo must not cost a decode.
    let presets = builtin_presets();
    let preset_font = presets
        .iter()
        .find(|p| p.name == opts.preset)
        .ok_or_else(|| OpsError::PresetNotFound { name: opts.preset.clone() })?
        .font
        .clone();

    let decoded = HwpxDecoder::decode(hwpx).map_err(OpsError::decode)?;
    let mut style_store = decoded.style_store;

    let Some(base) = style_store.iter_fonts().next().map(|f| f.face_name.clone()) else {
        return Err(OpsError::NoFonts);
    };
    style_store.replace_font(&base, &preset_font);

    let validated = decoded.document.validate()?;
    let sections = validated.sections().len();
    let paragraphs: usize = validated.sections().iter().map(|s| s.paragraphs.len()).sum();
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &validated,
        &style_store,
        &decoded.image_store,
        EncodeOptions::default(),
    )
    .map_err(OpsError::encode)?;
    let (bytes, warnings) = take_bytes_fail_closed(outcome)?;

    Ok(RestyleOutput { bytes, preset: opts.preset.clone(), sections, paragraphs, warnings })
}

// ── validate ────────────────────────────────────────────────────

/// What [`validate`] returns: a verdict, not a failure.
#[derive(Debug)]
#[non_exhaustive]
pub struct ValidateOutput {
    /// Whether the document passed `Document::validate`.
    pub ok: bool,
    /// Number of sections the package decoded into.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections.
    ///
    /// **Top-level only** — this is not the deep traversal
    /// [`inspect`](fn@super::inspect) reports. It is the number the MCP `validate` tool
    /// prints today, and it is here so a frontend does not have to decode
    /// the package a second time just to show a size alongside the verdict.
    pub paragraphs: usize,
    /// The validation errors; empty when `ok` is true.
    pub errors: Vec<WarningInfo>,
    /// Decode warnings raised on the way in.
    pub warnings: Vec<OpsWarning>,
}

impl ValidateOutput {
    /// The serialisable wire payload.
    #[must_use]
    pub fn meta(&self) -> ValidateReport {
        ValidateReport {
            ok: self.ok,
            sections: self.sections,
            paragraphs: self.paragraphs,
            errors: self.errors.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// The `validate` wire payload:
/// `{ "ok": …, "sections": …, "paragraphs": …, "errors": [ … ], "warnings": [ … ] }`.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct ValidateReport {
    /// Whether the document passed validation.
    pub ok: bool,
    /// Number of sections the package decoded into.
    pub sections: usize,
    /// Body-flow paragraphs summed over those sections (top-level only).
    pub paragraphs: usize,
    /// The validation errors; empty when `ok` is true.
    pub errors: Vec<WarningInfo>,
    /// Decode warnings raised on the way in.
    pub warnings: Vec<WarningInfo>,
}

/// Checks that a decoded HWPX document satisfies Core's invariants.
///
/// # A failed check is not an error
///
/// "This document is invalid" is the answer the caller asked for, so it
/// comes back as `ok: false` with the rendered error in `errors` under the
/// code `VALIDATION_FAILED`. Only a document that could not be *read* at all
/// is an [`OpsError`] — the caller asked a question about a document, and
/// undecodable bytes are not one.
///
/// # Why the counts come back too
///
/// `sections` and `paragraphs` are measured on the decoded document before
/// the verdict, so they are present whether the check passed or failed. They
/// let a frontend report "3 sections, 42 paragraphs, invalid" from the one
/// decode this function already paid for.
///
/// # Errors
///
/// [`OpsError::Decode`] (`DECODE_FAILED`) when the bytes are not a decodable
/// HWPX package.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::style::validate;
///
/// let bytes = std::fs::read("document.hwpx")?;
/// let out = validate(&bytes)?;
/// assert!(out.ok, "{:?}", out.errors);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn validate(hwpx: &[u8]) -> Result<ValidateOutput, OpsError> {
    let decoded = HwpxDecoder::decode(hwpx).map_err(OpsError::decode)?;
    let warnings: Vec<OpsWarning> = decoded.warnings.into_iter().map(OpsWarning::Decode).collect();

    // Measured before `validate` takes the document, so a failed verdict
    // still reports the size of what was read.
    let sections = decoded.document.sections().len();
    let paragraphs: usize = decoded.document.sections().iter().map(|s| s.paragraphs.len()).sum();

    match decoded.document.validate() {
        Ok(_) => {
            Ok(ValidateOutput { ok: true, sections, paragraphs, errors: Vec::new(), warnings })
        }
        Err(error) => Ok(ValidateOutput {
            ok: false,
            sections,
            paragraphs,
            errors: vec![WarningInfo::new(VALIDATION_FAILED, error.to_string())],
            warnings,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_preset_names_a_font_and_a_description() {
        let out = templates();

        assert!(!out.presets.is_empty(), "the preset table is compiled in");
        for preset in &out.presets {
            assert!(!preset.name.trim().is_empty());
            assert!(!preset.description.trim().is_empty());
            assert!(!preset.font.trim().is_empty(), "{} has no base font", preset.name);
        }
    }

    #[test]
    fn preset_names_are_unique_so_lookup_is_unambiguous() {
        let mut names: Vec<String> = templates().presets.into_iter().map(|p| p.name).collect();
        let total = names.len();
        names.sort();
        names.dedup();

        assert_eq!(names.len(), total, "restyle resolves a preset by name");
    }

    #[test]
    fn default_restyle_options_name_no_preset() {
        // A caller that forgets `with_preset` must fail loudly rather than
        // get an arbitrary house style applied to their document.
        assert!(RestyleOptions::default().preset.is_empty());
    }

    #[test]
    fn with_preset_is_the_only_way_to_set_one() {
        assert_eq!(RestyleOptions::default().with_preset("classic").preset, "classic");
    }

    // The `ok: false` branch is genuinely reachable, not dead code: a
    // section carrying no paragraphs decodes cleanly and then fails
    // `Document::validate` with `EmptySection`. That is proven against real
    // bytes in `smithy-hwpx`, by
    // `patch::tests::a_section_without_paragraphs_decodes_into_a_document_core_rejects`,
    // which is where the crate's only package writer lives.
    //
    // It cannot be driven end to end from here: no public API produces such
    // a package (`HwpxEncoder::encode` takes a `Document<Validated>`, so the
    // type state forbids it), and mutating one would need a ZIP writer this
    // crate does not depend on. Reaching it from `validate()` therefore
    // needs a committed fixture. Until then the failed-verdict *rendering*
    // is pinned here, where the output struct can be built directly.
    #[test]
    fn a_failed_verdict_renders_under_the_validation_failed_code() {
        let out = ValidateOutput {
            ok: false,
            sections: 2,
            paragraphs: 7,
            errors: vec![WarningInfo::new(VALIDATION_FAILED, "section 0 has no paragraphs")],
            warnings: Vec::new(),
        };

        let value = serde_json::to_value(out.meta()).expect("serialise");
        assert_eq!(value["ok"], false);
        assert_eq!(value["sections"], 2, "a failed verdict still reports what was read");
        assert_eq!(value["paragraphs"], 7);
        assert_eq!(value["errors"][0]["code"], "VALIDATION_FAILED");
        assert_eq!(value["errors"][0]["message"], "section 0 has no paragraphs");
        assert!(value["errors"][0].get("hint").is_none(), "no hint set, none serialised");
    }
}
