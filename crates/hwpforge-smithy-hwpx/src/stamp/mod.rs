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
mod detect;
mod plan;

pub use apply::{apply, StampAction, StampError, StampOutcome, StampSpec, StampedField};
pub use detect::{detect_markers, paragraph_guard, BuiltinPattern, GuardReason, MarkerHit};
pub use plan::{plan, StampCandidate};
