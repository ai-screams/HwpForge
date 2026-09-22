//! `stamp_plan` · `stamp` — promoting prose placeholders into named fields.
//!
//! Stamping is deliberately two-phase. [`stamp_plan`] discovers the
//! candidates a document offers and pins the source hash; a human or an
//! agent then authors an approval map that names or explicitly ignores every
//! unguarded candidate; [`stamp`] applies that map all-or-nothing.
//!
//! # Fail-closed
//!
//! Stamping regenerates the package, so it runs behind the admission gate
//! (no-op round-trip equality plus a ZIP closed-world check) and refuses to
//! emit bytes when the encode reports semantic loss. Both refusals live in
//! `smithy-hwpx`; this module only classifies them
//! (`ENCODE_SEMANTIC_LOSS` and the `INPUT_NOT_ROUNDTRIP_SAFE` family).
//!
//! # Two manifest shapes
//!
//! A legacy (v1) spec array and a v2 envelope produce different manifests —
//! `stamp` provenance is flat in v1 and origin-tagged in v2 — so
//! [`StampedManifest`] carries whichever the request produced instead of
//! normalising one into the other. `schema_version` discriminates them.
//!
//! # Manifest vs. apply-phase outcome
//!
//! [`StampOutput::manifest`] and [`StampOutput::stamped`] /
//! [`StampOutput::stamped_cells`] / [`StampOutput::ignored`] /
//! [`StampOutput::skipped_guarded`] answer different questions and are
//! independent of each other. The manifest lists the output document's
//! *whole* ClickHere field inventory, in document order, and tags only
//! *whether* a field is one this stamp created; it can be turned off with
//! [`StampOptions::with_manifest`]. The apply-phase fields are this
//! [`stamp`] call's disposition of every plan candidate — what got named
//! (spec order, the same [`StampedField`] / [`CellStampedField`] shape the
//! apply pass produced), what was explicitly ignored, and what stayed
//! untouched because it was guarded and no spec approved it — and they are
//! always populated, manifest or not. A legacy request never carries cell
//! specs, so [`StampOutput::stamped_cells`] is always empty on that path.
//!
//! # Warnings
//!
//! [`stamp_plan`] is a query: it decodes and projects, so it reports the
//! decoder's warnings ([`OpsWarning::Decode`]).
//!
//! [`stamp`] runs both halves of the codec, so it reports both, in that
//! order: **what decoding the input reported ([`OpsWarning::Decode`]), then
//! what the successful encode raised ([`OpsWarning::Encode`])**.
//!
//! The decode half is the admission gate's read of the input — the decode
//! whose document this stamp mutates, so it is the one that says what the
//! codec could not carry across the regeneration. Three further decodes run
//! and none is reported: the gate re-decodes its own no-op encode, the v2
//! fixed-point check decodes a re-encode, and the manifest re-reads the
//! output. Each reads a package that is either discarded or derived from the
//! decode already reported, so carrying them would report one document's
//! losses more than once.
//!
//! The encode half is the other half of the fail-closed contract: an encode
//! that loses meaning produces no bytes, and an encode that succeeds hands
//! back its **non-semantic** warnings. Only the encode that produced the
//! output is reported — the admission gate and the v2 fixed-point check also
//! encode, but those packages are discarded verification artefacts.
//!
//! That encode list is empty for every document available today: the only
//! non-semantic `EncodeWarning` is `LayoutCacheDropped`, which the encoder
//! raises only under `EncodeOptions::emit_layout_cache`, an opt-in a
//! preserve-first editor must never set. It is carried rather than discarded
//! so a future warning needs no API change.

use std::path::{Path, PathBuf};

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::stamp::{
    CellStampedField, HwpxStamper, StampManifest, StampManifestV2, StampMap, StampPlanV2,
    StampResult, StampResultV2, StampedField,
};
use serde::Serialize;

use super::{OpsError, OpsWarning};

/// One approved class-A (text) candidate — the request DTO `stamp` takes
/// (inside a [`StampMap::Legacy`] array, or `text` of its v2 envelope) and
/// every frontend's wire schema is built from.
///
/// W6b audit follow-up: a re-export of `smithy-hwpx`'s own type, not an
/// `ops`-owned mirror. `ops::stamp` consumes a whole [`StampMap`] as a plain
/// Rust value rather than parsing JSON into this type itself, so there is no
/// ops-side parsing step for a mirror struct to front — the frontends'
/// `#[derive(Deserialize)]` request structs decode straight into this type
/// today, and re-exporting it here just gives them one name to import
/// (`hwpforge::ops::StampSpec`) instead of reaching past `ops` into
/// `smithy-hwpx` directly. If `smithy-hwpx` ever renames or reshapes this
/// type, fix the break here; a schema snapshot test in `hwpforge-bindings-mcp`
/// pins the JSON shape so an incompatible change fails loudly there.
pub use hwpforge_smithy_hwpx::stamp::StampSpec;

/// One approved class-B (cell) target — the request DTO `stamp` takes
/// (inside the `cells` batch of a [`StampMap::V2`] envelope) and every
/// frontend's wire schema is built from.
///
/// W6b audit follow-up: same re-export rationale as [`StampSpec`] above —
/// see that doc comment.
pub use hwpforge_smithy_hwpx::stamp::CellStampSpec;

/// The manifest path a stamp writes when the caller supplies none.
///
/// Replaces `output`'s extension with `manifest.json` via
/// [`Path::with_extension`] — `form.hwpx` becomes `form.manifest.json`, and a
/// path with no extension at all gets one appended (`form` also becomes
/// `form.manifest.json`).
///
/// W6b audit follow-up: this is the CLI's pre-migration rule, now shared with
/// MCP. The two frontends disagreed on a doubly-suffixed path —
/// `Path::with_extension` only strips the *last* extension, so `form.hwpx.hwpx`
/// becomes `form.hwpx.manifest.json` here, while MCP's own pre-migration
/// version (`format!("{}.manifest.json", output.trim_end_matches(".hwpx"))`)
/// strips *every* trailing `.hwpx` — `str::trim_end_matches` repeats the
/// match — and collapsed the same input to `form.manifest.json`. Neither
/// frontend's audited legacy contract (`tests/data/legacy_codes.txt`; codes
/// and hints, not path arithmetic) pins either behaviour, and no known caller
/// passes a doubly-suffixed output path, so this migration keeps the CLI's
/// `Path`-based rule for both rather than compat-mapping the divergence.
#[must_use]
pub fn default_manifest_path(output: &Path) -> PathBuf {
    output.with_extension("manifest.json")
}

// ── stamp_plan ──────────────────────────────────────────────────

/// What [`stamp_plan`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct StampPlanOutput {
    /// Both candidate classes plus the source hash an approval map must pin.
    pub plan: StampPlanV2,
    /// Decoder warnings for this document, in decoder order.
    pub warnings: Vec<OpsWarning>,
}

impl StampPlanOutput {
    /// The serialisable form of this plan.
    #[must_use]
    pub fn meta(&self) -> StampPlanMeta {
        StampPlanMeta {
            plan: self.plan.clone(),
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Wire shape of a [`StampPlanOutput`].
///
/// The plan is flattened, so the object carries `schema_version`,
/// `source_sha256`, `text`, `cells` and `skipped_tables` beside `warnings`.
/// Deriving `Deserialize` or `JsonSchema` is not possible: [`StampPlanV2`]
/// is `Serialize`-only upstream.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct StampPlanMeta {
    /// The plan itself, inlined into this object.
    #[serde(flatten)]
    pub plan: StampPlanV2,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Enumerates the stamp candidates a document offers.
///
/// Discovery only: nothing is mutated and no admission gate runs. Copy
/// `source_sha256` verbatim into the approval map so [`stamp`] can reject a
/// map authored against a drifted document.
///
/// # Errors
///
/// [`OpsError::Stamper`] (code `STAMP_CODEC_FAILED`) when the bytes fail to
/// decode.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::ops::stamp::stamp_plan;
///
/// let bytes = std::fs::read("form.hwpx")?;
/// let out = stamp_plan(&bytes)?;
/// println!("{} text + {} cell candidates", out.plan.text.len(), out.plan.cells.len());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn stamp_plan(hwpx: &[u8]) -> Result<StampPlanOutput, OpsError> {
    let diagnosed = HwpxStamper::plan_bytes_v2_with_diagnostics(hwpx)?;
    Ok(StampPlanOutput {
        plan: diagnosed.value,
        warnings: diagnosed.warnings.into_iter().map(OpsWarning::Decode).collect(),
    })
}

// ── stamp ───────────────────────────────────────────────────────

/// Options for [`stamp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct StampOptions {
    /// Whether to return the manifest. The library always builds and
    /// validates it — this only decides whether the caller receives it, so
    /// turning it off saves the copy, never the work.
    pub manifest: bool,
}

impl Default for StampOptions {
    fn default() -> Self {
        Self { manifest: true }
    }
}

impl StampOptions {
    /// Returns (or omits) the manifest.
    #[must_use]
    pub fn with_manifest(mut self, manifest: bool) -> Self {
        self.manifest = manifest;
        self
    }
}

/// The manifest a stamp produced, in the shape its request asked for.
///
/// A legacy request yields the v1 shape and a v2 envelope the v2 shape;
/// `schema_version` tells them apart. The two are kept apart rather than
/// normalised because the manifest is a published JSON payload: v1
/// provenance is the flat `{pattern, marker, source_location, span}` object,
/// v2 provenance is the origin-tagged `{"text": {…}}` / `{"cell": {…}}`
/// union.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum StampedManifest {
    /// Manifest of a legacy (spec-array) stamp.
    V1(StampManifest),
    /// Manifest of a v2 (envelope) stamp.
    V2(StampManifestV2),
}

impl StampedManifest {
    /// The manifest schema version, which is also what discriminates the
    /// two shapes on the wire.
    #[must_use]
    pub fn schema_version(&self) -> u32 {
        match self {
            Self::V1(manifest) => manifest.schema_version,
            Self::V2(manifest) => manifest.schema_version,
        }
    }
}

/// What [`stamp`] returns.
#[derive(Debug)]
#[non_exhaustive]
pub struct StampOutput {
    /// The stamped HWPX package.
    pub bytes: Vec<u8>,
    /// The output inventory, unless [`StampOptions::with_manifest`] turned
    /// it off.
    pub manifest: Option<StampedManifest>,
    /// Class-A text fields created by this stamp, in spec order.
    ///
    /// Empty for a v2 request with no text specs. This is the apply-phase
    /// outcome, not a projection of `manifest`: the manifest lists the
    /// output's whole field inventory in document order and only tags
    /// *whether* each one was stamped, while this is spec order and exists
    /// independently of [`StampOptions::with_manifest`].
    pub stamped: Vec<StampedField>,
    /// Class-B cell fields created by this stamp, in spec order.
    ///
    /// Always empty for a legacy ([`StampMap::Legacy`]) request — cell
    /// specs only exist on the v2 path.
    pub stamped_cells: Vec<CellStampedField>,
    /// Number of explicitly ignored candidates, both classes combined.
    pub ignored: usize,
    /// Guarded candidates left untouched because no spec approved them,
    /// both classes combined.
    pub skipped_guarded: usize,
    /// What decoding the input reported, then the successful encode's
    /// non-semantic warnings — see the module docs for the order and for
    /// which of the codec passes on this path are reported.
    pub warnings: Vec<OpsWarning>,
}

impl StampOutput {
    /// The serialisable metadata of this stamp (everything but the bytes).
    #[must_use]
    pub fn meta(&self) -> StampMeta {
        StampMeta {
            manifest: self.manifest.clone(),
            stamped: self.stamped.clone(),
            stamped_cells: self.stamped_cells.clone(),
            ignored: self.ignored,
            skipped_guarded: self.skipped_guarded,
            warnings: self.warnings.iter().map(OpsWarning::info).collect(),
        }
    }
}

/// Wire shape of a [`StampOutput`] minus its bytes.
///
/// Deriving `Deserialize` or `JsonSchema` is not possible: both manifest
/// shapes are `Serialize`-only upstream.
#[derive(Debug, Clone, Serialize)]
#[non_exhaustive]
pub struct StampMeta {
    /// The output inventory; absent when the caller asked for no manifest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest: Option<StampedManifest>,
    /// Class-A text fields created by this stamp, in spec order.
    pub stamped: Vec<StampedField>,
    /// Class-B cell fields created by this stamp, in spec order.
    pub stamped_cells: Vec<CellStampedField>,
    /// Number of explicitly ignored candidates, both classes combined.
    pub ignored: usize,
    /// Guarded candidates left untouched because no spec approved them,
    /// both classes combined.
    pub skipped_guarded: usize,
    /// Non-fatal diagnostics.
    pub warnings: Vec<WarningInfo>,
}

/// Applies an approved stamp map, all-or-nothing.
///
/// Build `request` with
/// [`parse_stamp_map`](hwpforge_smithy_hwpx::stamp::parse_stamp_map), which
/// accepts both the legacy spec array and the v2 envelope and validates the
/// v2 contract (schema version, source-hash shape, non-blank cell hints).
/// A v2 request additionally pins `source_sha256` against the input, so a
/// map authored for a different revision is refused before anything is
/// touched.
///
/// # Errors
///
/// - [`OpsError::Stamper`] for every library rejection: the admission
///   refusals (`INPUT_NOT_ROUNDTRIP_SAFE`, `INPUT_ENTRIES_NOT_CARRIED`),
///   drift (`STAMP_SOURCE_HASH_MISMATCH`, `STAMP_LABEL_DRIFT`,
///   `STAMP_SPEC_STALE`), coverage (`STAMP_CANDIDATE_UNCOVERED`), naming
///   (`STAMP_NAME_DUPLICATE`, `STAMP_NAME_COLLISION`, `STAMP_NAME_EMPTY`),
///   the post-encode self-verification (`STAMP_DELTA_MISMATCH`) and the
///   fail-closed `ENCODE_SEMANTIC_LOSS`.
/// - `INVALID_STAMP_MAP` is **not** reachable from here: parsing happens in
///   the caller, so a malformed map fails before this function is entered.
///
/// # Examples
///
/// ```no_run
/// use hwpforge::hwpx::stamp::parse_stamp_map;
/// use hwpforge::ops::stamp::{stamp, StampOptions};
///
/// let bytes = std::fs::read("form.hwpx")?;
/// let request = parse_stamp_map(&std::fs::read_to_string("map.json")?)?;
/// let out = stamp(&bytes, &request, &StampOptions::default())?;
/// assert!(out.manifest.is_some());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn stamp(
    hwpx: &[u8],
    request: &StampMap,
    opts: &StampOptions,
) -> Result<StampOutput, OpsError> {
    #[allow(clippy::type_complexity)]
    let (
        bytes,
        manifest,
        stamped,
        stamped_cells,
        ignored,
        skipped_guarded,
        decode_warnings,
        encode_warnings,
    ) = match request {
        StampMap::Legacy(specs) => {
            let diagnosed = HwpxStamper::stamp_with_diagnostics(hwpx, specs)?;
            let StampResult { bytes, manifest, outcome } = diagnosed.value;
            (
                bytes,
                StampedManifest::V1(manifest),
                outcome.stamped,
                Vec::new(),
                outcome.ignored,
                outcome.skipped_guarded.len(),
                diagnosed.decode_warnings,
                diagnosed.encode_warnings,
            )
        }
        StampMap::V2(envelope) => {
            let diagnosed = HwpxStamper::stamp_v2_with_diagnostics(hwpx, envelope)?;
            let StampResultV2 { bytes, manifest, outcome } = diagnosed.value;
            (
                bytes,
                StampedManifest::V2(manifest),
                outcome.text.stamped,
                outcome.cells.stamped,
                outcome.text.ignored + outcome.cells.ignored,
                outcome.text.skipped_guarded.len() + outcome.cells.skipped_guarded.len(),
                diagnosed.decode_warnings,
                diagnosed.encode_warnings,
            )
        }
    };
    // Documented order: the input decode first, then the encode that produced
    // the output.
    let mut warnings: Vec<OpsWarning> =
        decode_warnings.into_iter().map(OpsWarning::Decode).collect();
    warnings.extend(encode_warnings.into_iter().map(OpsWarning::Encode));
    Ok(StampOutput {
        bytes,
        manifest: opts.manifest.then_some(manifest),
        stamped,
        stamped_cells,
        ignored,
        skipped_guarded,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_is_returned_unless_it_is_turned_off() {
        assert!(StampOptions::default().manifest, "callers expect a manifest by default");
        assert!(!StampOptions::default().with_manifest(false).manifest);
        assert!(StampOptions::default().with_manifest(false).with_manifest(true).manifest);
    }

    #[test]
    fn default_manifest_path_replaces_a_single_extension() {
        assert_eq!(
            default_manifest_path(Path::new("form.hwpx")),
            PathBuf::from("form.manifest.json")
        );
    }

    #[test]
    fn default_manifest_path_strips_only_the_last_extension() {
        // The divergence the doc comment describes: only the trailing
        // `.hwpx` is replaced, not every `.hwpx` suffix.
        assert_eq!(
            default_manifest_path(Path::new("form.hwpx.hwpx")),
            PathBuf::from("form.hwpx.manifest.json")
        );
    }

    #[test]
    fn default_manifest_path_appends_when_there_is_no_extension() {
        assert_eq!(default_manifest_path(Path::new("form")), PathBuf::from("form.manifest.json"));
    }
}
