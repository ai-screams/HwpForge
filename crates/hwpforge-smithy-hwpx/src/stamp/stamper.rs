//! Byte-level stamping facade (E6 Wave 1B surface): admission-gated
//! decode → apply → encode over a complete `.hwpx`, plus the manifest.
//!
//! Fail-closed (design §3-1/§3-6): an input is admitted only when it is
//! provably round-trip-safe for THIS codec — a no-op decode→encode→decode
//! must reproduce the same Core semantics and the output package must carry
//! every input ZIP entry. Anything else is rejected with no output; file
//! names and provenance are never used as a safety proxy.

use std::collections::HashSet;
use std::io::Cursor;

use serde::Serialize;

use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::Control;

use super::apply::{apply, StampAction, StampError, StampOutcome, StampSpec};
use super::apply_cells::{commit_cells, preflight_cells, CellStampError, CellStampOutcome};
use super::plan::{plan, StampCandidate};
use super::request::{CellLabelClaim, StampRequestV2};
use crate::decoder::{HwpxDecoder, HwpxDocument};
use crate::encoder::HwpxEncoder;
use crate::fill::{FieldInfo, HwpxFiller};
use crate::patch::sha256_hex;
use crate::table_inventory::for_each_table_mut;

/// Manifest schema version (bump on breaking manifest shape changes).
pub const STAMP_MANIFEST_VERSION: u32 = 1;

/// v2 manifest schema version (text+cell origins, tagged `stamp` enum).
pub const STAMP_MANIFEST_V2_VERSION: u32 = 2;

/// Byte-level stamping entry point.
///
/// See [`HwpxStamper::plan_bytes`] (discovery) and [`HwpxStamper::stamp`]
/// (admission-gated apply + manifest).
pub struct HwpxStamper;

/// Result of a successful [`HwpxStamper::stamp`].
#[derive(Debug)]
pub struct StampResult {
    /// The stamped `.hwpx` bytes.
    pub bytes: Vec<u8>,
    /// The manifest describing every field in the output.
    pub manifest: StampManifest,
    /// Apply-phase outcome (stamped/ignored/skipped-guarded).
    pub outcome: StampOutcome,
}

/// Machine-readable inventory of the stamped output (design §3-5).
///
/// Invariant: `fields[*].field` is the re-decoded OUTPUT's `list_fields`
/// result verbatim — never a projection estimated during planning.
#[derive(Debug, Clone, Serialize)]
pub struct StampManifest {
    /// Manifest schema version ([`STAMP_MANIFEST_VERSION`]).
    pub schema_version: u32,
    /// SHA-256 (hex) of the input bytes.
    pub source_sha256: String,
    /// SHA-256 (hex) of the output bytes.
    pub output_sha256: String,
    /// Every ClickHere field in the output document, in document order.
    pub fields: Vec<ManifestField>,
}

/// One field in the output inventory.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestField {
    /// The field as `list_fields` reports it (name/hint/current/section/
    /// fillable).
    #[serde(flatten)]
    pub field: FieldInfo,
    /// Stamping metadata — present only for fields this stamp created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<StampMeta>,
}

/// Provenance of a stamped field.
#[derive(Debug, Clone, Serialize)]
pub struct StampMeta {
    /// Detector id (`builtin:checkbox`, `builtin:paren_blank`, …).
    pub pattern: String,
    /// Original marker text the field replaced.
    pub marker: String,
    /// Pre-stamp semantic path (`source_location` — addresses the ORIGINAL
    /// document; run indices shift after the split).
    pub source_location: String,
    /// UTF-8 byte span of the marker within the original slot text.
    pub span: (usize, usize),
}

/// [`HwpxStamper`] failure — fail-closed, no output bytes on error.
#[derive(Debug)]
#[non_exhaustive]
pub enum StamperError {
    /// Decode/encode/validate failure while processing the input.
    Codec(String),
    /// The input is not round-trip-safe for this codec: a no-op
    /// decode→encode→decode changed Core semantics. `component` names the
    /// first differing store (`document` / `style_store` / `image_store`)
    /// and `diff_path` the first differing JSON path within it.
    NotRoundTripSafe {
        /// Which decoded component differs.
        component: String,
        /// First differing path (serde projection).
        diff_path: String,
    },
    /// The encoder does not carry these input ZIP entries — stamping would
    /// silently drop them (closed-world admission, design §3-6②).
    UncarriedZipEntries {
        /// Input entry names missing from the re-encoded package.
        entries: Vec<String>,
    },
    /// Apply-phase preflight rejection.
    Stamp(StampError),
    /// The output inventory violates the manifest invariant (duplicate or
    /// missing names, or a stamped field that is not fillable).
    ManifestInvariant {
        /// Human-readable description.
        detail: String,
    },
    /// The v2 map's `source_sha256` does not match the input bytes — the
    /// map was authored against a different (drifted) document.
    SourceHashMismatch {
        /// Hash the map claimed.
        expected: String,
        /// Hash of the actual input.
        actual: String,
    },
    /// Cell-apply preflight rejection.
    CellStamp(CellStampError),
    /// Post-encode verification failure: the re-decoded output does not
    /// reduce back to the pre-stamp document (reverse delta), or is not an
    /// encode/decode fixed point.
    DeltaMismatch {
        /// Which verification stage failed (`reverse_delta` / `fixed_point`
        /// / `stamped_cell`).
        stage: String,
        /// First differing component or a description of the failure.
        detail: String,
    },
}

impl std::fmt::Display for StamperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codec(msg) => write!(f, "codec failure: {msg}"),
            Self::NotRoundTripSafe { component, diff_path } => write!(
                f,
                "input is not round-trip-safe: {component} differs at {diff_path} after a \
                 no-op decode→encode→decode — refusing to stamp (E4 preserve-first path \
                 required for this input)"
            ),
            // Review L2: entry names come from an untrusted archive — debug
            // formatting escapes control characters (terminal injection).
            Self::UncarriedZipEntries { entries } => write!(
                f,
                "input ZIP entries not carried by the encoder (would be dropped): {entries:?}"
            ),
            Self::Stamp(e) => write!(f, "stamp preflight: {e}"),
            Self::ManifestInvariant { detail } => write!(f, "manifest invariant: {detail}"),
            Self::SourceHashMismatch { expected, actual } => write!(
                f,
                "source_sha256 mismatch: map was authored for {expected}, input is {actual} — \
                 re-run plan on the current document"
            ),
            Self::CellStamp(e) => write!(f, "cell stamp preflight: {e}"),
            Self::DeltaMismatch { stage, detail } => {
                write!(f, "post-encode verification failed at {stage}: {detail}")
            }
        }
    }
}

impl std::error::Error for StamperError {}

impl From<StampError> for StamperError {
    fn from(e: StampError) -> Self {
        Self::Stamp(e)
    }
}

impl From<CellStampError> for StamperError {
    fn from(e: CellStampError) -> Self {
        Self::CellStamp(e)
    }
}

impl HwpxStamper {
    /// Decodes the input and enumerates class-A placeholder candidates.
    ///
    /// Discovery only — no admission gate, no mutation. Use the returned
    /// candidates to author the spec map for [`HwpxStamper::stamp`].
    ///
    /// # Errors
    ///
    /// [`StamperError::Codec`] when the input fails to decode.
    pub fn plan_bytes(base: &[u8]) -> Result<Vec<StampCandidate>, StamperError> {
        let decoded = HwpxDecoder::decode(base).map_err(|e| StamperError::Codec(e.to_string()))?;
        Ok(plan(&decoded.document))
    }

    /// Stamps the input with the approved specs, all-or-nothing.
    ///
    /// Pipeline: admission gate (no-op round-trip + ZIP closed-world) →
    /// [`apply`] → encode → re-decode → manifest (output `list_fields` is
    /// the source of truth). Every failure is fail-closed: no bytes are
    /// produced.
    ///
    /// # Errors
    ///
    /// See [`StamperError`].
    pub fn stamp(base: &[u8], specs: &[StampSpec]) -> Result<StampResult, StamperError> {
        // ── admission gate ──────────────────────────────────────────
        let d0 = HwpxDecoder::decode(base).map_err(|e| StamperError::Codec(e.to_string()))?;
        let e0 = encode_hwpx(&d0)?;
        let d1 = HwpxDecoder::decode(&e0).map_err(|e| StamperError::Codec(e.to_string()))?;
        admission_compare(&d0, &d1)?;
        check_zip_carry(base, &e0)?;

        // ── apply on the admitted decode ────────────────────────────
        let HwpxDocument { mut document, style_store, image_store } = d0;
        let outcome = apply(&mut document, specs)?;
        let validated =
            document.validate().map_err(|e| StamperError::Codec(format!("validate: {e}")))?;
        let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)
            .map_err(|e| StamperError::Codec(e.to_string()))?;

        // ── manifest from the OUTPUT (re-decode is the source of truth) ─
        let fields = HwpxFiller::list_fields(&bytes)
            .map_err(|e| StamperError::Codec(format!("output list_fields: {e}")))?;
        let manifest = build_manifest(base, &bytes, &fields, &outcome)?;
        Ok(StampResult { bytes, manifest, outcome })
    }

    /// Stamps a v2 request (class-A text + class-B cells), all-or-nothing.
    ///
    /// Pipeline: source-hash pinning → admission gate → unified preflight
    /// (cell preflight against the pristine document, cross-class name
    /// reservation) → text apply → cell commit → encode → re-decode →
    /// post-encode verification (per-cell expected field, reverse cell
    /// delta against a text-only reference round-trip, output fixed point)
    /// → v2 manifest. Fail-closed: no bytes on any error.
    ///
    /// # Errors
    ///
    /// See [`StamperError`] — notably [`StamperError::SourceHashMismatch`],
    /// [`StamperError::CellStamp`], and [`StamperError::DeltaMismatch`].
    pub fn stamp_v2(base: &[u8], request: &StampRequestV2) -> Result<StampResultV2, StamperError> {
        // ── source-hash pinning (drift rejection) ───────────────────
        let actual = sha256_hex(base);
        if !actual.eq_ignore_ascii_case(&request.source_sha256) {
            return Err(StamperError::SourceHashMismatch {
                expected: request.source_sha256.clone(),
                actual,
            });
        }

        // ── admission gate (identical to v1) ────────────────────────
        let d0 = HwpxDecoder::decode(base).map_err(|e| StamperError::Codec(e.to_string()))?;
        let e0 = encode_hwpx(&d0)?;
        let d1 = HwpxDecoder::decode(&e0).map_err(|e| StamperError::Codec(e.to_string()))?;
        admission_compare(&d0, &d1)?;
        check_zip_carry(base, &e0)?;

        // ── unified preflight, then mutate ──────────────────────────
        let HwpxDocument { mut document, style_store, image_store } = d0;
        let reserved: HashSet<String> = request
            .text
            .iter()
            .filter_map(|s| match &s.action {
                StampAction::Field { name, .. } => Some(name.clone()),
                StampAction::Ignore => None,
            })
            .collect();
        // Cell preflight runs against the pristine document; text apply is
        // internally atomic; cell commit is mechanical — so a failure at
        // any point leaves no partially-stamped output (combined
        // atomicity).
        let cell_plan = preflight_cells(&mut document, &request.cells, &reserved)?;
        let text_outcome = apply(&mut document, &request.text)?;
        // Text-only reference for the reverse-delta gate (before cells).
        let text_only = HwpxDocument {
            document: document.clone(),
            style_store: style_store.clone(),
            image_store: image_store.clone(),
        };
        let cell_outcome = commit_cells(&mut document, cell_plan);

        let validated =
            document.validate().map_err(|e| StamperError::Codec(format!("validate: {e}")))?;
        let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)
            .map_err(|e| StamperError::Codec(e.to_string()))?;

        // ── post-encode verification on the re-decoded OUTPUT ───────
        let d2 = HwpxDecoder::decode(&bytes)
            .map_err(|e| StamperError::Codec(format!("output decode: {e}")))?;
        verify_and_reverse_cells(&d2, &text_only, &cell_outcome)?;
        let e2 = encode_hwpx(&d2)?;
        let d3 = HwpxDecoder::decode(&e2)
            .map_err(|e| StamperError::Codec(format!("fixed-point decode: {e}")))?;
        admission_compare(&d2, &d3).map_err(|e| StamperError::DeltaMismatch {
            stage: "fixed_point".to_string(),
            detail: e.to_string(),
        })?;

        // ── v2 manifest from the OUTPUT ─────────────────────────────
        let fields = HwpxFiller::list_fields(&bytes)
            .map_err(|e| StamperError::Codec(format!("output list_fields: {e}")))?;
        let manifest = build_manifest_v2(base, &bytes, &fields, &text_outcome, &cell_outcome)?;
        Ok(StampResultV2 {
            bytes,
            manifest,
            outcome: StampOutcomeV2 { text: text_outcome, cells: cell_outcome },
        })
    }
}

/// Verifies each stamped cell in the re-decoded output and reverses the
/// cell delta, comparing against the text-only reference round-trip.
///
/// Both sides of the comparison have been through exactly one
/// encode→decode, so codec normalization cancels and any residual
/// difference is a real stamping defect.
fn verify_and_reverse_cells(
    d2: &HwpxDocument,
    text_only: &HwpxDocument,
    cells: &CellStampOutcome,
) -> Result<(), StamperError> {
    let mut reverted = d2.document.clone();
    let mut pending: std::collections::HashMap<usize, Vec<&super::apply_cells::CellStampedField>> =
        std::collections::HashMap::new();
    for stamped in &cells.stamped {
        pending.entry(stamped.table).or_default().push(stamped);
    }
    let mut failure: Option<StamperError> = None;
    for_each_table_mut(&mut reverted, &mut |ordinal, table| {
        let Some(edits) = pending.remove(&ordinal) else {
            return;
        };
        if failure.is_some() {
            return;
        }
        let Ok(grid) = hwpforge_core::table::grid::TableGrid::from_table(table) else {
            failure = Some(StamperError::DeltaMismatch {
                stage: "stamped_cell".to_string(),
                detail: format!("table {ordinal}: output grid invalid"),
            });
            return;
        };
        for stamped in edits {
            let Some(anchor) = grid.resolve(stamped.at).copied() else {
                failure = Some(stamped_cell_failure(stamped, "cell missing from output grid"));
                return;
            };
            let cell = &mut table.rows[anchor.row_idx].cells[anchor.cell_idx];
            let Some(run) = cell.paragraphs.first_mut().and_then(|p| p.runs.first_mut()) else {
                failure = Some(stamped_cell_failure(stamped, "cell has no runs in output"));
                return;
            };
            let is_expected = matches!(
                &run.content,
                RunContent::Control(control) if matches!(
                    control.as_ref(),
                    Control::Field { name: Some(name), .. } if *name == stamped.name
                )
            );
            if !is_expected {
                failure = Some(stamped_cell_failure(stamped, "expected named ClickHere not found"));
                return;
            }
            *run = Run::text(&stamped.original_text, run.char_shape_id);
        }
    });
    if let Some(err) = failure {
        return Err(err);
    }

    let reverted = HwpxDocument {
        document: reverted,
        style_store: d2.style_store.clone(),
        image_store: d2.image_store.clone(),
    };
    // Reference: the text-only document after ONE encode→decode round trip.
    let reference_bytes = encode_hwpx(text_only)?;
    let reference = HwpxDecoder::decode(&reference_bytes)
        .map_err(|e| StamperError::Codec(format!("reference decode: {e}")))?;
    admission_compare(&reverted, &reference).map_err(|e| StamperError::DeltaMismatch {
        stage: "reverse_delta".to_string(),
        detail: e.to_string(),
    })
}

fn stamped_cell_failure(
    stamped: &super::apply_cells::CellStampedField,
    detail: &str,
) -> StamperError {
    StamperError::DeltaMismatch {
        stage: "stamped_cell".to_string(),
        detail: format!(
            "table {} ({},{}) field {:?}: {detail}",
            stamped.table, stamped.at.row, stamped.at.col, stamped.name
        ),
    }
}

/// Result of a successful [`HwpxStamper::stamp_v2`].
#[derive(Debug)]
pub struct StampResultV2 {
    /// The stamped `.hwpx` bytes.
    pub bytes: Vec<u8>,
    /// The v2 manifest describing every field in the output.
    pub manifest: StampManifestV2,
    /// Apply-phase outcomes for both classes.
    pub outcome: StampOutcomeV2,
}

/// Combined apply outcome of a v2 stamp.
#[derive(Debug)]
pub struct StampOutcomeV2 {
    /// Class-A text outcome.
    pub text: StampOutcome,
    /// Class-B cell outcome.
    pub cells: CellStampOutcome,
}

/// Machine-readable v2 inventory of the stamped output.
///
/// Same invariant as v1: `fields[*]` (minus `stamp`) is the re-decoded
/// OUTPUT's `list_fields` result verbatim.
#[derive(Debug, Clone, Serialize)]
pub struct StampManifestV2 {
    /// Manifest schema version ([`STAMP_MANIFEST_V2_VERSION`]).
    pub schema_version: u32,
    /// SHA-256 (hex) of the input bytes.
    pub source_sha256: String,
    /// SHA-256 (hex) of the output bytes.
    pub output_sha256: String,
    /// Every ClickHere field in the output document, in document order.
    pub fields: Vec<ManifestFieldV2>,
}

/// One field in the v2 output inventory.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestFieldV2 {
    /// The field as `list_fields` reports it.
    #[serde(flatten)]
    pub field: FieldInfo,
    /// Stamping provenance — present only for fields this stamp created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<StampOriginV2>,
}

/// Provenance of a v2-stamped field, tagged by origin class.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StampOriginV2 {
    /// Class-A: promoted from an inline text marker.
    Text {
        /// Detector id (`builtin:checkbox`, …).
        pattern: String,
        /// Original marker text the field replaced.
        marker: String,
        /// Pre-stamp semantic path (addresses the ORIGINAL document).
        source_location: String,
        /// UTF-8 byte span of the marker within the original slot text.
        span: (usize, usize),
    },
    /// Class-B: promoted from a stampable empty table cell.
    Cell {
        /// Table ordinal (shared inventory DFS pre-order).
        table: usize,
        /// Anchor coordinate of the stamped cell.
        at: GridCoord,
        /// The approved label claim (`None` = explicit orphan).
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<CellLabelClaim>,
    },
}

pub(crate) fn encode_hwpx(doc: &HwpxDocument) -> Result<Vec<u8>, StamperError> {
    let validated = doc
        .document
        .clone()
        .validate()
        .map_err(|e| StamperError::Codec(format!("validate: {e}")))?;
    HwpxEncoder::encode(&validated, &doc.style_store, &doc.image_store)
        .map_err(|e| StamperError::Codec(e.to_string()))
}

pub(crate) fn admission_compare(a: &HwpxDocument, b: &HwpxDocument) -> Result<(), StamperError> {
    if a.document != b.document {
        return Err(StamperError::NotRoundTripSafe {
            component: "document".to_string(),
            diff_path: first_diff_path(&a.document, &b.document),
        });
    }
    if a.style_store != b.style_store {
        return Err(StamperError::NotRoundTripSafe {
            component: "style_store".to_string(),
            diff_path: first_diff_path(&a.style_store, &b.style_store),
        });
    }
    if a.image_store != b.image_store {
        return Err(StamperError::NotRoundTripSafe {
            component: "image_store".to_string(),
            diff_path: "(image payloads)".to_string(),
        });
    }
    Ok(())
}

/// First differing JSON path between two serializable values.
fn first_diff_path<T: Serialize>(a: &T, b: &T) -> String {
    fn walk(a: &serde_json::Value, b: &serde_json::Value, path: &str) -> Option<String> {
        use serde_json::Value;
        match (a, b) {
            (Value::Object(ma), Value::Object(mb)) => {
                let mut keys: Vec<&String> = ma.keys().chain(mb.keys()).collect();
                keys.sort();
                keys.dedup();
                for k in keys {
                    let na = ma.get(k).unwrap_or(&Value::Null);
                    let nb = mb.get(k).unwrap_or(&Value::Null);
                    if let Some(p) = walk(na, nb, &format!("{path}.{k}")) {
                        return Some(p);
                    }
                }
                None
            }
            (Value::Array(va), Value::Array(vb)) => {
                if va.len() != vb.len() {
                    return Some(format!("{path} (len {} vs {})", va.len(), vb.len()));
                }
                for (i, (ea, eb)) in va.iter().zip(vb).enumerate() {
                    if let Some(p) = walk(ea, eb, &format!("{path}[{i}]")) {
                        return Some(p);
                    }
                }
                None
            }
            _ => (a != b).then(|| path.to_string()),
        }
    }
    let va = serde_json::to_value(a).unwrap_or(serde_json::Value::Null);
    let vb = serde_json::to_value(b).unwrap_or(serde_json::Value::Null);
    walk(&va, &vb, "$").unwrap_or_else(|| "(equal under serde projection)".to_string())
}

/// Closed-world ZIP check: every input entry must exist in the re-encoded
/// package, or stamping would silently drop it.
pub(crate) fn check_zip_carry(base: &[u8], encoded: &[u8]) -> Result<(), StamperError> {
    let base_entries = zip_entry_names(base)?;
    let out_entries = zip_entry_names(encoded)?;
    let missing: Vec<String> =
        base_entries.into_iter().filter(|name| !out_entries.contains(name)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(StamperError::UncarriedZipEntries { entries: missing })
    }
}

fn zip_entry_names(bytes: &[u8]) -> Result<Vec<String>, StamperError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| StamperError::Codec(format!("zip: {e}")))?;
    let mut names = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let entry =
            archive.by_index(i).map_err(|e| StamperError::Codec(format!("zip entry {i}: {e}")))?;
        // Directory placeholders are packaging detail, not content.
        if !entry.name().ends_with('/') {
            names.push(entry.name().to_string());
        }
    }
    Ok(names)
}

fn build_manifest(
    base: &[u8],
    output: &[u8],
    fields: &[FieldInfo],
    outcome: &StampOutcome,
) -> Result<StampManifest, StamperError> {
    // Invariant checks: stamped names exist exactly once, named, fillable.
    for stamped in &outcome.stamped {
        let matches: Vec<&FieldInfo> =
            fields.iter().filter(|f| f.name.as_deref() == Some(stamped.name.as_str())).collect();
        match matches.as_slice() {
            [one] => {
                if !one.fillable {
                    return Err(StamperError::ManifestInvariant {
                        detail: format!("stamped field {:?} is not fillable", stamped.name),
                    });
                }
            }
            [] => {
                return Err(StamperError::ManifestInvariant {
                    detail: format!("stamped field {:?} missing from output", stamped.name),
                })
            }
            _ => {
                return Err(StamperError::ManifestInvariant {
                    detail: format!("stamped field {:?} appears more than once", stamped.name),
                })
            }
        }
    }

    let fields = fields
        .iter()
        .map(|f| ManifestField {
            field: f.clone(),
            stamp: f.name.as_deref().and_then(|name| {
                outcome.stamped.iter().find(|s| s.name == name).map(|s| StampMeta {
                    pattern: format!("builtin:{}", s.pattern.id()),
                    marker: s.marker.clone(),
                    source_location: s.path.clone(),
                    span: (s.span.start, s.span.end),
                })
            }),
        })
        .collect();

    Ok(StampManifest {
        schema_version: STAMP_MANIFEST_VERSION,
        source_sha256: sha256_hex(base),
        output_sha256: sha256_hex(output),
        fields,
    })
}

fn check_stamped_name(fields: &[FieldInfo], name: &str) -> Result<(), StamperError> {
    let matches: Vec<&FieldInfo> =
        fields.iter().filter(|f| f.name.as_deref() == Some(name)).collect();
    match matches.as_slice() {
        [one] if one.fillable => Ok(()),
        [_] => Err(StamperError::ManifestInvariant {
            detail: format!("stamped field {name:?} is not fillable"),
        }),
        [] => Err(StamperError::ManifestInvariant {
            detail: format!("stamped field {name:?} missing from output"),
        }),
        _ => Err(StamperError::ManifestInvariant {
            detail: format!("stamped field {name:?} appears more than once"),
        }),
    }
}

fn build_manifest_v2(
    base: &[u8],
    output: &[u8],
    fields: &[FieldInfo],
    text: &StampOutcome,
    cells: &CellStampOutcome,
) -> Result<StampManifestV2, StamperError> {
    for stamped in &text.stamped {
        check_stamped_name(fields, &stamped.name)?;
    }
    for stamped in &cells.stamped {
        check_stamped_name(fields, &stamped.name)?;
    }

    let fields = fields
        .iter()
        .map(|f| ManifestFieldV2 {
            field: f.clone(),
            stamp: f.name.as_deref().and_then(|name| {
                if let Some(s) = text.stamped.iter().find(|s| s.name == name) {
                    return Some(StampOriginV2::Text {
                        pattern: format!("builtin:{}", s.pattern.id()),
                        marker: s.marker.clone(),
                        source_location: s.path.clone(),
                        span: (s.span.start, s.span.end),
                    });
                }
                cells.stamped.iter().find(|s| s.name == name).map(|s| StampOriginV2::Cell {
                    table: s.table,
                    at: s.at,
                    label: s.label.clone(),
                })
            }),
        })
        .collect();

    Ok(StampManifestV2 {
        schema_version: STAMP_MANIFEST_V2_VERSION,
        source_sha256: sha256_hex(base),
        output_sha256: sha256_hex(output),
        fields,
    })
}

#[cfg(test)]
mod tests {
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::run::Run;
    use hwpforge_core::{Document, Paragraph, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

    use super::*;
    use crate::style_store::HwpxStyleStore;

    fn doc_with_text(text: &str) -> HwpxDocument {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::default(),
        ));
        HwpxDocument {
            document: doc,
            style_store: HwpxStyleStore::with_default_fonts("함초롬돋움"),
            image_store: ImageStore::new(),
        }
    }

    #[test]
    fn admission_accepts_identical_documents() {
        let a = doc_with_text("같음");
        let b = doc_with_text("같음");
        assert!(admission_compare(&a, &b).is_ok());
    }

    #[test]
    fn admission_reports_first_document_diff_path() {
        let a = doc_with_text("원본");
        let b = doc_with_text("변형");
        let err = admission_compare(&a, &b).unwrap_err();
        match err {
            StamperError::NotRoundTripSafe { component, diff_path } => {
                assert_eq!(component, "document");
                assert!(
                    diff_path.contains("sections[0]"),
                    "diff path must locate the change: {diff_path}"
                );
            }
            other => panic!("expected NotRoundTripSafe, got {other}"),
        }
    }

    #[test]
    fn first_diff_path_reports_array_length_changes() {
        let a = vec![1, 2, 3];
        let b = vec![1, 2];
        let path = first_diff_path(&a, &b);
        assert!(path.contains("len 3 vs 2"), "got {path}");
    }
}
