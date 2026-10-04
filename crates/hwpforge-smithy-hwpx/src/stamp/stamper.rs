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
use super::cells::plan_cells;
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

/// 인코드 + **의미 손상 fail-closed** (7차 평결 High).
///
/// 편집기(stamper/cell-edit)는 preserve-first 계약이다 — 인코더가
/// [`crate::EncodeWarning::NoteHeadSkipped`] 등 의미 손상 경고를 낸 산출물을
/// 무경고 반환하면, admission 은 Core 비교라 이를 못 본다 (디코더가
/// FOOTNOTE/ENDNOTE autoNum 을 Core 로 올리지 않아 native fused 번호
/// 머리는 재인코드에서 `AlreadyPresent` 가 될 수 없다 — titleMark 각주의
/// 가시 번호가 무음 삭제되는 실경로). public 결과형 변경 없이 기존
/// `Codec` 오류로 거부한다 — 경고를 결과에 싣는 richer API 는 별도 승인
/// 대상 (계획 문서 §7i).
///
/// 어떤 경고가 의미 손상인지는 [`crate::EncodeWarning::is_semantic_loss`]
/// 가 단독으로 정의한다 (편집기별 손수 목록 금지).
/// R2 HIGH 1: 성공 시 **비의미 경고를 함께** 돌려준다 (과거엔 바이트만
/// 남기고 버렸다 — `*_with_diagnostics` 트윈이 이 목록을 싣는다).
fn encode_fail_closed(
    validated: &hwpforge_core::Document<hwpforge_core::Validated>,
    style_store: &crate::style_store::HwpxStyleStore,
    image_store: &hwpforge_core::image::ImageStore,
) -> Result<(Vec<u8>, Vec<crate::EncodeWarning>), StamperError> {
    let outcome = HwpxEncoder::encode_with_diagnostics(
        validated,
        style_store,
        image_store,
        crate::EncodeOptions::default(),
    )
    .map_err(|e| StamperError::Codec(e.to_string()))?;
    // R1 F4: typed 경고를 실어 돌려준다 (과거엔 `Codec(String)` 으로 뭉개져
    // 호출자가 어떤 경고였는지 알 수 없었다). Display 문자열은 불변이라
    // 프론트엔드 출력은 그대로다.
    crate::encoder::split_successful_encode(outcome)
        .map_err(|(warnings, others)| StamperError::SemanticLoss { warnings, others })
}

/// Result of a successful [`HwpxStamper::stamp`].
///
/// 현재 인코드 [`EncodeWarning`](crate::EncodeWarning) (각주 번호 머리
/// 생략 등)은 이 결과에 실리지 않는다 — 경고 채널 신설은 public 필드
/// 추가(semver) 라 별도 승인 대기 (각주 에픽 계획 문서 §7h).
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
    /// 인코드가 의미 손상 경고를 내 스탬핑을 거부했다 (fail-closed — 바이트 없음).
    ///
    /// 분류의 정의는 [`crate::EncodeWarning::is_semantic_loss`] 하나뿐이다.
    /// 두 벡터 모두 인코더가 낸 **원래 순서**를 유지하며, 이 오류가 구성될
    /// 때 `warnings` 는 비어 있지 않다.
    SemanticLoss {
        /// 의미 손상 경고 전부 (원래 순서, 비어 있지 않음).
        warnings: Vec<crate::EncodeWarning>,
        /// 같은 인코드가 낸 나머지 경고 (원래 순서) — 진단용으로 함께 싣는다.
        others: Vec<crate::EncodeWarning>,
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
            // 이 Display 는 **이 타입의 프론트엔드가 이미 찍고 있는 문자열**과
            // 같아야 한다. CLI/MCP stamp 는 `Codec(msg)` 의 **안쪽 문자열**을
            // 그대로 메시지로 썼으므로 여기에는 "codec failure: " 접두사가
            // 없다 (로그 드리프트 0).
            //
            // ⚠️ `CellEditError::SemanticLoss` 는 접두사가 **있다** — 그쪽
            // 프론트엔드는 `error.to_string()` 을 쓰기 때문이다. 둘을 "통일"
            // 하면 한쪽 출력이 바뀐다.
            Self::SemanticLoss { warnings, .. } => match warnings.first() {
                Some(w) => {
                    write!(f, "encode produced a semantic-loss warning (fail-closed): {w}")
                }
                None => write!(f, "encode produced a semantic-loss warning (fail-closed)"),
            },
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

    /// Decodes the input and enumerates BOTH candidate classes (v2 plan).
    ///
    /// Discovery only — no admission gate, no mutation. The returned
    /// `source_sha256` must be copied verbatim into the v2 map so
    /// [`HwpxStamper::stamp_v2`] can reject drifted inputs.
    ///
    /// # Errors
    ///
    /// [`StamperError::Codec`] when the input fails to decode.
    pub fn plan_bytes_v2(base: &[u8]) -> Result<StampPlanV2, StamperError> {
        Self::plan_bytes_v2_with_diagnostics(base)
            .map(crate::diagnostics::WithDecodeWarnings::into_value)
    }

    /// Enumerates both candidate classes, keeping the decoder's warnings.
    ///
    /// Same plan as [`HwpxStamper::plan_bytes_v2`], which is a thin wrapper
    /// over this function. Planning is a **query** — it decodes and projects,
    /// mutating nothing — so the decoder's diagnostics are the only ones this
    /// path can produce.
    ///
    /// # Errors
    ///
    /// [`StamperError::Codec`] when the input fails to decode.
    pub fn plan_bytes_v2_with_diagnostics(
        base: &[u8],
    ) -> Result<crate::diagnostics::WithDecodeWarnings<StampPlanV2>, StamperError> {
        let decoded = HwpxDecoder::decode(base).map_err(|e| StamperError::Codec(e.to_string()))?;
        let cells = plan_cells(&decoded.document);
        Ok(crate::diagnostics::WithDecodeWarnings::new(
            StampPlanV2 {
                schema_version: super::request::STAMP_MAP_VERSION,
                source_sha256: sha256_hex(base),
                text: plan(&decoded.document),
                cells: cells.candidates,
                skipped_tables: cells.skipped_tables,
            },
            decoded.warnings,
        ))
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
    /// See [`StamperError`]. The layout-cache carry step
    /// decompresses every package entry, including ones the decoder never
    /// reads, so a package with an entry over 50 MiB, or over 500 MiB in total,
    /// is refused.
    pub fn stamp(base: &[u8], specs: &[StampSpec]) -> Result<StampResult, StamperError> {
        Self::stamp_with_diagnostics(base, specs)
            .map(crate::diagnostics::WithCodecWarnings::into_value)
    }

    /// Stamps the input, keeping both codec channels: what decoding the input
    /// reported, and the successful encode's non-semantic warnings.
    ///
    /// Same bytes, manifest and outcome as [`HwpxStamper::stamp`], which is a
    /// thin wrapper over this function.
    ///
    /// # What the decode warnings mean
    ///
    /// They come from the admission gate's read of `base` — the decode whose
    /// document this stamp actually mutates, so it is the one that says what
    /// the codec could not carry across the regeneration.
    ///
    /// Three other decodes run on this path and none of them is reported: the
    /// admission gate re-decodes its own no-op encode (a discarded artefact),
    /// and the manifest re-reads the **output** through
    /// [`HwpxFiller::list_fields`]. That last one is the line the review
    /// pointed at; it reads bytes produced from the very decode already
    /// reported, so its diagnostics can only restate a subset of them, and
    /// carrying both would report one document's losses twice.
    ///
    /// # What the encode warnings mean
    ///
    /// Semantic-loss warnings never appear here — they are the fail-closed
    /// path and produce [`StamperError::SemanticLoss`] with no bytes.
    ///
    /// The list comes from the single encode that produced the output. The
    /// admission gate's no-op round trip encodes too, but that output is
    /// discarded, so reporting its diagnostics would describe a package the
    /// caller never receives. The warnings are also captured **before** the
    /// layout-cache carry step, so
    /// [`EncodeWarning::LayoutCacheDropped`](crate::EncodeWarning::LayoutCacheDropped)
    /// means "this encode did not emit that cache", not "the output lacks
    /// it" — see [`HwpxCellEditor::set_cells_with_diagnostics`](crate::HwpxCellEditor::set_cells_with_diagnostics)
    /// for the same contract stated in full.
    ///
    /// # Errors
    ///
    /// See [`StamperError`]. The layout-cache carry step
    /// decompresses every package entry, including ones the decoder never
    /// reads, so a package with an entry over 50 MiB, or over 500 MiB in total,
    /// is refused.
    pub fn stamp_with_diagnostics(
        base: &[u8],
        specs: &[StampSpec],
    ) -> Result<crate::diagnostics::WithCodecWarnings<StampResult>, StamperError> {
        // ── admission gate ──────────────────────────────────────────
        let d0 = HwpxDecoder::decode(base).map_err(|e| StamperError::Codec(e.to_string()))?;
        let e0 = encode_hwpx(&d0)?;
        let d1 = HwpxDecoder::decode(&e0).map_err(|e| StamperError::Codec(e.to_string()))?;
        admission_compare(&d0, &d1)?;
        check_zip_carry(base, &e0)?;
        // After the older checks, so an input they already refused keeps
        // their error code.
        reject_input_content_loss(&d0.warnings)?;

        // ── apply on the admitted decode ────────────────────────────
        let HwpxDocument { mut document, style_store, image_store, warnings: decode_warnings } = d0;
        let outcome = apply(&mut document, specs)?;
        let validated =
            document.validate().map_err(|e| StamperError::Codec(format!("validate: {e}")))?;
        let (bytes, encode_warnings) = encode_fail_closed(&validated, &style_store, &image_store)?;
        // Untouched paragraphs keep Hancom's line-layout cache (no full
        // document reflow/repagination in the renderer).
        let bytes = crate::layout_carry::carry_line_segs(base, &e0, &bytes)
            .map_err(|e| StamperError::Codec(format!("layout carry: {e}")))?;

        // ── manifest from the OUTPUT (re-decode is the source of truth) ─
        let fields = HwpxFiller::list_fields(&bytes)
            .map_err(|e| StamperError::Codec(format!("output list_fields: {e}")))?;
        let manifest = build_manifest(base, &bytes, &fields, &outcome)?;
        Ok(crate::diagnostics::WithCodecWarnings::new(
            StampResult { bytes, manifest, outcome },
            decode_warnings,
            encode_warnings,
        ))
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
        Self::stamp_v2_with_diagnostics(base, request)
            .map(crate::diagnostics::WithCodecWarnings::into_value)
    }

    /// Stamps a v2 request, keeping both codec channels.
    ///
    /// Same bytes, manifest and outcome as [`HwpxStamper::stamp_v2`], which
    /// is a thin wrapper over this function. The contract for both channels
    /// is the one [`HwpxStamper::stamp_with_diagnostics`] documents; v2
    /// additionally encodes and decodes during the fixed-point check, and
    /// those diagnostics are likewise not reported, because that package is a
    /// discarded verification artefact rather than the result.
    ///
    /// # Errors
    ///
    /// See [`StamperError`] — notably [`StamperError::SourceHashMismatch`],
    /// [`StamperError::CellStamp`], and [`StamperError::DeltaMismatch`].
    pub fn stamp_v2_with_diagnostics(
        base: &[u8],
        request: &StampRequestV2,
    ) -> Result<crate::diagnostics::WithCodecWarnings<StampResultV2>, StamperError> {
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
        // After the older checks, so an input they already refused keeps
        // their error code.
        reject_input_content_loss(&d0.warnings)?;

        // ── unified preflight, then mutate ──────────────────────────
        let HwpxDocument { mut document, style_store, image_store, warnings: decode_warnings } = d0;
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
            warnings: Vec::new(),
            document: document.clone(),
            style_store: style_store.clone(),
            image_store: image_store.clone(),
        };
        let cell_outcome = commit_cells(&mut document, cell_plan);

        let validated =
            document.validate().map_err(|e| StamperError::Codec(format!("validate: {e}")))?;
        let (bytes, encode_warnings) = encode_fail_closed(&validated, &style_store, &image_store)?;
        // Untouched paragraphs keep Hancom's line-layout cache (no full
        // document reflow/repagination in the renderer).
        let bytes = crate::layout_carry::carry_line_segs(base, &e0, &bytes)
            .map_err(|e| StamperError::Codec(format!("layout carry: {e}")))?;

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
        Ok(crate::diagnostics::WithCodecWarnings::new(
            StampResultV2 {
                bytes,
                manifest,
                outcome: StampOutcomeV2 { text: text_outcome, cells: cell_outcome },
            },
            decode_warnings,
            encode_warnings,
        ))
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
        warnings: Vec::new(),
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

/// Result of [`HwpxStamper::plan_bytes_v2`]: both candidate classes plus
/// the source hash the approval map must pin.
#[derive(Debug, Clone, Serialize)]
pub struct StampPlanV2 {
    /// The map schema version this plan feeds
    /// ([`super::request::STAMP_MAP_VERSION`]).
    pub schema_version: u32,
    /// SHA-256 (hex) of the input bytes — copy into the v2 map verbatim.
    pub source_sha256: String,
    /// Class-A text candidates (same shape as [`HwpxStamper::plan_bytes`]).
    pub text: Vec<StampCandidate>,
    /// Class-B cell candidates.
    pub cells: Vec<super::cells::CellStampCandidate>,
    /// Tables excluded from cell detection (invalid grid) — explicit
    /// incomplete-coverage diagnostics.
    pub skipped_tables: Vec<super::cells::SkippedTable>,
}

/// Result of a successful [`HwpxStamper::stamp_v2`].
///
/// 의미 손상 인코드 경고(번호 머리/titleMark 생략 등)는 결과에 실리는 대신
/// [`StamperError::Codec`] 으로 **fail-closed** 된다 (`encode_fail_closed`).
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

/// Refuses an input whose decode reported lost content
/// ([`DecodeWarning::is_content_loss`](crate::DecodeWarning::is_content_loss)).
///
/// The editors regenerate the whole package from the decode, so they would
/// write that loss into their output. The no-op admission round trip cannot
/// see it: both of its decodes lose the same thing. The first such warning is
/// named in the error.
pub(crate) fn reject_input_content_loss(
    warnings: &[crate::DecodeWarning],
) -> Result<(), StamperError> {
    match warnings.iter().find(|w| w.is_content_loss()) {
        Some(crate::DecodeWarning::ContentLost { part, path, kind, .. }) => {
            Err(StamperError::NotRoundTripSafe {
                component: format!("input decode: CONTENT_LOST {kind}"),
                diff_path: format!("{part}:{path}"),
            })
        }
        Some(other) => Err(StamperError::NotRoundTripSafe {
            component: format!("input decode: {}", other.code()),
            diff_path: other.location().unwrap_or_default(),
        }),
        None => Ok(()),
    }
}

pub(crate) fn admission_compare(a: &HwpxDocument, b: &HwpxDocument) -> Result<(), StamperError> {
    // 캐시 정규화 비교: 줄 조판 캐시(layout_cache)는 Hancom 이 저장한 렌더
    // 캐시라 원본(보유)과 우리 재인코드 산출물(미보유)이 당연히 다르다.
    // 문서 의미 동등성 판정에서 제외하기 위해 양쪽 사본에서 제거 후 비교한다.
    let mut doc_a = a.document.clone();
    let mut doc_b = b.document.clone();
    doc_a.strip_layout_caches();
    doc_b.strip_layout_caches();
    if doc_a != doc_b {
        // A shape's exact Core value need not survive HWPX storage digit for
        // digit (whole-degree angles, integer sizes): a shape that lands
        // within the exactness tolerance of where it was is the same shape.
        let va = serde_json::to_value(&doc_a).unwrap_or(serde_json::Value::Null);
        let mut vb = serde_json::to_value(&doc_b).unwrap_or(serde_json::Value::Null);
        align_equivalent_shapes(&va, &mut vb);
        if va != vb {
            return Err(StamperError::NotRoundTripSafe {
                component: "document".to_string(),
                diff_path: first_diff_path(&va, &vb),
            });
        }
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

/// Shape kinds whose geometry is compared within tolerance.
const SHAPE_KINDS: [&str; 9] =
    ["TextBox", "Rect", "Ellipse", "Arc", "Polygon", "Curve", "ConnectLine", "Line", "Group"];

/// Walks two serialized documents in step and, wherever both hold the same
/// kind of shape at the same place whose box corners and defining points lie
/// within 2 HWPUNIT of each other (each in its own parent's coordinates),
/// copies `a`'s geometry fields into `b` — so only a real difference is left
/// for the equality check. Everything else about the shapes must still match.
fn align_equivalent_shapes(a: &serde_json::Value, b: &mut serde_json::Value) {
    use serde_json::Value;
    let one_shape = |v: &Value| match v {
        Value::Object(m) if m.len() == 1 => {
            m.keys().next().filter(|k| SHAPE_KINDS.contains(&k.as_str())).cloned()
        }
        _ => None,
    };
    if let (Some(ka), Some(kb)) = (one_shape(a), one_shape(b)) {
        if ka == kb {
            align_shape(a, b);
        }
    }
    match (a, b) {
        (Value::Object(ma), Value::Object(mb)) => {
            for (k, va) in ma {
                if let Some(vb) = mb.get_mut(k) {
                    align_equivalent_shapes(va, vb);
                }
            }
        }
        (Value::Array(va), Value::Array(vb)) if va.len() == vb.len() => {
            for (ea, eb) in va.iter().zip(vb.iter_mut()) {
                align_equivalent_shapes(ea, eb);
            }
        }
        _ => {}
    }
}

/// Replaces `b` (one serialized shape control) with `b` carrying `a`'s
/// geometry when the two are the same shape within tolerance.
fn align_shape(a: &serde_json::Value, b: &mut serde_json::Value) {
    let (Ok(ca), Ok(cb)) = (
        serde_json::from_value::<Control>(a.clone()),
        serde_json::from_value::<Control>(b.clone()),
    ) else {
        return;
    };
    if !same_place(&ca, &cb) {
        return;
    }
    let aligned = with_geometry_of(cb, &ca);
    if let Ok(v) = serde_json::to_value(&aligned) {
        *b = v;
    }
}

/// Where a shape's box corners and defining points land in its parent's
/// coordinates (exact Core definition).
fn placed_points(c: &Control) -> Option<Vec<(f64, f64)>> {
    use hwpforge_core::control::geometry::{emitted_corners, local_transform, EmissionPolicy};
    let exact = EmissionPolicy::exact();
    let mut out = emitted_corners(c, &exact)?.to_vec();
    let l = local_transform(c, &exact)?;
    let (x, y) = shape_offset(c);
    out.extend(defining_points(c).iter().map(|p| {
        let (px, py) = l.apply((f64::from(p.x), f64::from(p.y)));
        (px + f64::from(x), py + f64::from(y))
    }));
    Some(out)
}

fn same_place(a: &Control, b: &Control) -> bool {
    if std::mem::discriminant(a) != std::mem::discriminant(b) {
        return false;
    }
    match (placed_points(a), placed_points(b)) {
        (Some(pa), Some(pb)) if pa.len() == pb.len() => pa.iter().zip(&pb).all(|(p, q)| {
            (p.0 - q.0).hypot(p.1 - q.1) <= crate::decoder::group_geometry::EXACT_TOLERANCE
        }),
        _ => false,
    }
}

fn shape_offset(c: &Control) -> (i32, i32) {
    let placement = match c {
        Control::TextBox { placement, .. }
        | Control::Rect { placement, .. }
        | Control::Ellipse { placement, .. }
        | Control::Arc { placement, .. }
        | Control::Polygon { placement, .. }
        | Control::Curve { placement, .. }
        | Control::ConnectLine { placement, .. }
        | Control::Line { placement, .. }
        | Control::Group { placement, .. } => placement,
        _ => return (0, 0),
    };
    placement.as_ref().map_or((0, 0), |p| (p.horz_offset.as_i32(), p.vert_offset.as_i32()))
}

fn defining_points(c: &Control) -> Vec<hwpforge_core::control::ShapePoint> {
    match c {
        Control::Line { start, end, .. } => vec![*start, *end],
        Control::Ellipse { center, axis1, axis2, .. } => vec![*center, *axis1, *axis2],
        Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } => {
            vec![*center, *axis1, *axis2, *start1, *end1, *start2, *end2]
        }
        Control::Polygon { vertices, .. } => vertices.clone(),
        Control::Curve { points, .. } => points.clone(),
        Control::ConnectLine { start, end, control_points, .. } => {
            let mut v = vec![*start, *end];
            v.extend(control_points.iter().copied());
            v
        }
        _ => Vec::new(),
    }
}

/// `b` with `a`'s size, offset, rotation, flip and defining points (`a` and
/// `b` are the same kind).
fn with_geometry_of(mut b: Control, a: &Control) -> Control {
    use hwpforge_core::control::ShapeStyle;
    type Geo<'c> = (
        Option<(&'c mut hwpforge_foundation::HwpUnit, &'c mut hwpforge_foundation::HwpUnit)>,
        Option<&'c mut Option<hwpforge_core::placement::ObjectPlacement>>,
        Option<&'c mut Option<ShapeStyle>>,
    );
    fn parts(c: &mut Control) -> Geo<'_> {
        match c {
            Control::TextBox { width, height, placement, style, .. }
            | Control::Rect { width, height, placement, style, .. }
            | Control::Ellipse { width, height, placement, style, .. }
            | Control::Arc { width, height, placement, style, .. }
            | Control::Polygon { width, height, placement, style, .. }
            | Control::Curve { width, height, placement, style, .. }
            | Control::ConnectLine { width, height, placement, style, .. }
            | Control::Line { width, height, placement, style, .. } => {
                (Some((width, height)), Some(placement), Some(style))
            }
            Control::Group { width, height, placement, .. } => {
                (Some((width, height)), Some(placement), None)
            }
            _ => (None, None, None),
        }
    }
    let mut a = a.clone();
    let points_a = defining_points(&a);
    {
        let (size_a, place_a, style_a) = parts(&mut a);
        let (size_b, place_b, style_b) = parts(&mut b);
        if let (Some((wa, ha)), Some((wb, hb))) = (size_a, size_b) {
            *wb = *wa;
            *hb = *ha;
        }
        if let (Some(pa), Some(pb)) = (place_a, place_b) {
            match (pa.as_ref(), pb.as_mut()) {
                (Some(pa), Some(pb)) => {
                    pb.horz_offset = pa.horz_offset;
                    pb.vert_offset = pa.vert_offset;
                }
                // A group child at its parent's corner has no placement; one
                // a unit or two away has the group-child form. Only that form
                // is matched, so an inline shape never equals a floating one.
                (None, Some(q)) if is_group_child_form(q) => *pb = None,
                (Some(p), None) if is_group_child_form(p) => *pb = Some(p.clone()),
                _ => {}
            }
        }
        if let (Some(sa), Some(sb)) = (style_a, style_b) {
            let (rotation, flip) = sa.as_ref().map_or((None, None), |s| (s.rotation, s.flip));
            let mut s = sb.take().unwrap_or_default();
            s.rotation = rotation;
            s.flip = flip;
            // A style that only held the rotation goes away with it.
            *sb = (sa.is_some() || s != ShapeStyle::default()).then_some(s);
        }
    }
    let mut pts = points_a.into_iter();
    for p in defining_points_mut(&mut b) {
        if let Some(q) = pts.next() {
            *p = q;
        }
    }
    b
}

/// The placement the group decoders give a child away from its parent's
/// corner (floating in front of text, paper-relative, overlap allowed).
fn is_group_child_form(p: &hwpforge_core::placement::ObjectPlacement) -> bool {
    use hwpforge_core::placement::{ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
    p.text_wrap == ObjectTextWrap::InFrontOfText
        && p.text_flow == ObjectTextFlow::BothSides
        && !p.treat_as_char
        && !p.flow_with_text
        && p.allow_overlap
        && p.vert_rel_to == ObjectRelativeTo::Paper
        && p.horz_rel_to == ObjectRelativeTo::Paper
}

fn defining_points_mut(c: &mut Control) -> Vec<&mut hwpforge_core::control::ShapePoint> {
    match c {
        Control::Line { start, end, .. } => vec![start, end],
        Control::Ellipse { center, axis1, axis2, .. } => vec![center, axis1, axis2],
        Control::Arc { center, axis1, axis2, start1, end1, start2, end2, .. } => {
            vec![center, axis1, axis2, start1, end1, start2, end2]
        }
        Control::Polygon { vertices, .. } => vertices.iter_mut().collect(),
        Control::Curve { points, .. } => points.iter_mut().collect(),
        Control::ConnectLine { start, end, control_points, .. } => {
            let mut v = vec![start, end];
            v.extend(control_points.iter_mut());
            v
        }
        _ => Vec::new(),
    }
}

/// First differing JSON path between two serializable values.
pub(crate) fn first_diff_path<T: Serialize>(a: &T, b: &T) -> String {
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
            warnings: Vec::new(),
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

    fn shapes_fixture(name: &str) -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/shapes/");
        std::fs::read(format!("{path}{name}")).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
    }

    /// `decode(bytes)` and the document of `decode(encode(decode(bytes)))`
    /// with the first decode's stores (Hancom's style store does not round
    /// trip yet; only the document is under test here).
    fn no_op_round_trip(bytes: &[u8]) -> (HwpxDocument, HwpxDocument) {
        let d0 = HwpxDecoder::decode(bytes).unwrap();
        let d1 = HwpxDecoder::decode(&encode_hwpx(&d0).unwrap()).unwrap();
        let d1 = HwpxDocument {
            warnings: Vec::new(),
            document: d1.document,
            style_store: d0.style_store.clone(),
            image_store: d0.image_store.clone(),
        };
        (d0, d1)
    }

    fn first_group_children_mut(doc: &mut HwpxDocument) -> &mut Vec<Control> {
        doc.document
            .sections_mut()
            .iter_mut()
            .flat_map(|s| s.paragraphs.iter_mut())
            .flat_map(|p| p.runs.iter_mut())
            .find_map(|r| match &mut r.content {
                RunContent::Control(c) => match c.as_mut() {
                    Control::Group { children, .. } => Some(children),
                    _ => None,
                },
                _ => None,
            })
            .expect("a group")
    }

    /// Hancom's group turned 20° and mirrored (rotflip-group): the folded
    /// angles are exact (9.99767° for the rect), HWPX stores whole degrees,
    /// and the re-decode reads 9.99999° — a different Core value for the
    /// same shape, well inside 2 HWPUNIT. Admission accepts it.
    ///
    /// 이것을 실패시키는 것: 도형 기하를 허용 오차 없이 완전 일치로 비교하는 것
    /// (`align_equivalent_shapes` 호출 삭제).
    #[test]
    fn admission_accepts_a_shape_within_the_tolerance() {
        let (d0, d1) = no_op_round_trip(&shapes_fixture("rotflip-group.hwpx"));
        let mut a = d0.document.clone();
        let mut b = d1.document.clone();
        a.strip_layout_caches();
        b.strip_layout_caches();
        assert_ne!(a, b, "the oracle: the exact Core values differ after the round trip");
        admission_compare(&d0, &d1).unwrap();
    }

    /// A child moved 3 HWPUNIT is a different shape; a child given another
    /// size likewise. Both are still refused.
    ///
    /// 이것을 실패시키는 것: 허용 오차를 무한으로 두는 것, 또는 기하 외 필드까지
    /// 덮어써서 비교하는 것.
    #[test]
    fn admission_refuses_a_shape_beyond_the_tolerance() {
        let bytes = shapes_fixture("rotflip-group.hwpx");
        let (d0, _) = no_op_round_trip(&bytes);
        let mut moved = no_op_round_trip(&bytes).1;
        if let Some(Control::Rect { placement: Some(p), .. }) =
            first_group_children_mut(&mut moved).iter_mut().find(|c| c.is_rect())
        {
            p.horz_offset = hwpforge_foundation::HwpUnit::new(p.horz_offset.as_i32() + 3).unwrap();
        } else {
            panic!("a floating rect child");
        }
        assert!(admission_compare(&d0, &moved).is_err());

        let mut recoloured = no_op_round_trip(&bytes).1;
        if let Some(Control::Rect { style: Some(s), .. }) =
            first_group_children_mut(&mut recoloured).iter_mut().find(|c| c.is_rect())
        {
            s.line_width = Some(s.line_width.unwrap_or(0) + 100);
        } else {
            panic!("a styled rect child");
        }
        assert!(admission_compare(&d0, &recoloured).is_err());
    }

    fn doc_with_rect(placement: Option<hwpforge_core::placement::ObjectPlacement>) -> HwpxDocument {
        let mut doc = doc_with_text("x");
        let unit = |v| hwpforge_foundation::HwpUnit::new(v).unwrap();
        let mut rect = Control::rect(unit(1000), unit(500)).unwrap();
        if let Control::Rect { placement: p, .. } = &mut rect {
            *p = placement;
        }
        doc.document.sections_mut()[0].paragraphs[0]
            .runs
            .push(Run::control(rect, hwpforge_foundation::CharShapeIndex::new(0)));
        doc
    }

    /// An inline shape and a floating one a unit away are not the same
    /// shape: only the group-child placement form may stand for the origin.
    ///
    /// 이것을 실패시키는 것: 배치 정렬에서 묶음 자식 형태 검사
    /// (`is_group_child_form`)를 지우는 것.
    #[test]
    fn admission_keeps_inline_and_floating_apart() {
        use hwpforge_core::placement::ObjectPlacement;
        let floating = ObjectPlacement {
            treat_as_char: false,
            horz_offset: hwpforge_foundation::HwpUnit::new(1).unwrap(),
            ..ObjectPlacement::legacy_inline_defaults()
        };
        assert!(admission_compare(&doc_with_rect(None), &doc_with_rect(Some(floating))).is_err());
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
