//! Template stamping (E6): promote prose placeholders to named ClickHere
//! fields so downstream [`fill`](crate::HwpxFiller) becomes deterministic.
//!
//! Wave 1A scope — class-A inline text markers only (checkbox glyphs,
//! whitespace-only paren blanks, date blanks, standalone `@`, seal/sign
//! tokens). Cell-position placeholders (class B) are gated behind E3 table
//! addressing; example-content placeholders (class C) are never auto-detected
//! and require explicit caller rules.
//!
//! Design contract (2026-07-20 design review):
//! - detection is a **closed** built-in pattern list — no semantic-word
//!   heuristics ("작성"/"기재" are NOT positive signals)
//! - candidates in instruction context (`※`/`【작성방법】`/`(예시)`) are
//!   downgraded to guarded and never auto-applied
//! - two-phase plan/apply: all rules produce a candidate plan, the whole
//!   plan preflights, then applies atomically

mod apply;
mod apply_cells;
mod cells;
mod detect;
mod plan;
mod request;
mod stamper;

pub use apply::{apply, StampAction, StampError, StampOutcome, StampSpec, StampedField};
pub use apply_cells::{apply_cells, CellStampError, CellStampOutcome, CellStampedField};
pub use cells::{plan_cells, CellPlan, CellStampCandidate, LabelDirection, LabelRef, SkippedTable};
pub use detect::{detect_markers, paragraph_guard, BuiltinPattern, GuardReason, MarkerHit};
pub use plan::{plan, StampCandidate};
pub use request::{
    parse_stamp_map, CellLabelClaim, CellStampAction, CellStampSpec, StampMap, StampMapError,
    StampRequestV2, STAMP_MAP_VERSION,
};
// E3 cell editing reuses the stamp admission gate (decode→encode→decode
// no-op equality + ZIP closed-world) so both mutation facades share one
// fail-closed contract.
pub(crate) use stamper::{admission_compare, check_zip_carry, encode_hwpx, first_diff_path};
pub use stamper::{
    HwpxStamper, ManifestField, ManifestFieldV2, StampManifest, StampManifestV2, StampMeta,
    StampOriginV2, StampOutcomeV2, StampPlanV2, StampResult, StampResultV2, StamperError,
    STAMP_MANIFEST_V2_VERSION, STAMP_MANIFEST_VERSION,
};
