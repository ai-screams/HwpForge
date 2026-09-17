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
//! # Warnings
//!
//! [`stamp_plan`] is a query: it decodes and projects, so it reports the
//! decoder's warnings ([`OpsWarning::Decode`]).
//!
//! [`stamp`] is a regenerating edit, so it reports the other half of its
//! fail-closed contract ([`OpsWarning::Encode`]): an encode that loses
//! meaning produces no bytes, and an encode that succeeds hands back its
//! **non-semantic** warnings. Only the encode that produced the output is
//! reported — the admission gate and the v2 fixed-point check also encode,
//! but those packages are discarded verification artefacts.
//!
//! That encode list is empty for every document available today: the only
//! non-semantic `EncodeWarning` is `LayoutCacheDropped`, which the encoder
//! raises only under `EncodeOptions::emit_layout_cache`, an opt-in a
//! preserve-first editor must never set. It is carried rather than discarded
//! so a future warning needs no API change.

use hwpforge_foundation::diagnostics::WarningInfo;
use hwpforge_smithy_hwpx::stamp::{
    HwpxStamper, StampManifest, StampManifestV2, StampMap, StampPlanV2,
};
use serde::Serialize;

use super::{OpsError, OpsWarning};

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
    /// Non-fatal diagnostics. See the module docs for why this is empty.
    pub warnings: Vec<OpsWarning>,
}

impl StampOutput {
    /// The serialisable metadata of this stamp (everything but the bytes).
    #[must_use]
    pub fn meta(&self) -> StampMeta {
        StampMeta {
            manifest: self.manifest.clone(),
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
    let (bytes, manifest, encode_warnings) = match request {
        StampMap::Legacy(specs) => {
            let diagnosed = HwpxStamper::stamp_with_diagnostics(hwpx, specs)?;
            (
                diagnosed.value.bytes,
                StampedManifest::V1(diagnosed.value.manifest),
                diagnosed.warnings,
            )
        }
        StampMap::V2(envelope) => {
            let diagnosed = HwpxStamper::stamp_v2_with_diagnostics(hwpx, envelope)?;
            (
                diagnosed.value.bytes,
                StampedManifest::V2(diagnosed.value.manifest),
                diagnosed.warnings,
            )
        }
    };
    Ok(StampOutput {
        bytes,
        manifest: opts.manifest.then_some(manifest),
        warnings: encode_warnings.into_iter().map(OpsWarning::Encode).collect(),
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
}
