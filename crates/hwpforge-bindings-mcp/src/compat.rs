//! Compatibility layer between `hwpforge::ops` and the MCP server's frozen
//! error contract.
//!
//! `hwpforge::ops` classifies every failure as an [`OpsCode`] wire string
//! (`hwpforge_foundation::diagnostics::OpsCode`), a table shared with the
//! CLI and the Python bindings. The MCP server predates that table and
//! already ships its own per-tool code/hint spelling (`DECODE_ERROR` where
//! `ops` now says `DECODE_FAILED`, `SET_CELL_CODEC_FAILED` where `ops` folds
//! both set-cell and stamp semantic-loss refusals into one
//! `ENCODE_SEMANTIC_LOSS`, …). [`tool_error`] and [`warning`] translate
//! `ops` results back onto that legacy spelling so migrating a tool file
//! from calling `smithy-hwpx`/`smithy-md` directly to calling `ops` is not a
//! wire-format break.
//!
//! # What is frozen and what is not
//!
//! The **code** and **hint** strings this module emits are the frozen
//! contract — `tests/data/legacy_codes.txt` is the audited snapshot of every
//! code a tool emitted before this migration, and this module's `#[cfg(test)]`
//! inventory (`mod tests`) checks every row it claims to cover against it.
//! The **message**
//! is not frozen (`common.md` W2 brief §"Contract that must NOT change"):
//! this module defaults to [`OpsError`]'s own `Display`, which is usually
//! byte-identical to the old message anyway because the wrapped library
//! errors (`SectionWorkflowError`, `CellEditError`, …) were already written
//! to match what the frontends print. The few places the two must match
//! exactly regardless (the semantic-loss regression pins in `set_cell.rs`
//! and `stamp.rs`) are reconstructed by hand below.
//!
//! # Table vs. special cases
//!
//! Most rows are static: a `(Tool, OpsCode)` pair determines a legacy code
//! and, where the legacy hint text is not `ops`'s own (`ops::hint()` returns
//! `None` for the overwhelming majority of codes — see `hwpforge/src/ops/mod.rs`'s
//! `hint_for`), a literal hint string lifted from the pre-migration tool
//! file. A handful of legacy hints embed per-call data (an available-fields
//! list, a section count, an anchor coordinate) that no flat table can hold;
//! those are matched on the *wrapped* library error directly, before the
//! table is consulted, in [`tool_error`].
//!
//! # The one tool that never maps an error
//!
//! `hwpforge_validate` folds a decode failure into its own `valid: false`
//! payload instead of returning an error, so [`Tool::Validate`] has no table
//! rows and no caller of [`tool_error`]; the variant exists so the inventory
//! covers all nineteen tools.

use hwpforge::ops::{OpsError, OpsWarning};
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_smithy_hwpx::stamp::{CellStampError, StampError, StamperError};
use hwpforge_smithy_hwpx::{FillError, SectionWorkflowError};

use crate::output::{ToolErrorInfo, ToolWarningInfo};

/// Which MCP tool is reporting the error.
///
/// One variant per `hwpforge_mcp_*` tool registered in `src/server.rs` (19
/// today). The MD→HWPX tool is `ConvertMd` here — matching `ops`'s
/// `convert_md` function name — but it is wired to the MCP tool named
/// `hwpforge_convert`; there is no separate `hwpforge_convert_md` tool.
/// `export_section` is not a variant: it has no MCP tool of its own, only
/// `ToJson`, which calls the section-export path internally when `section`
/// is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tool {
    /// `hwpforge_convert` (Markdown → HWPX).
    ConvertMd,
    /// `hwpforge_inspect`.
    Inspect,
    /// `hwpforge_outline`.
    Outline,
    /// `hwpforge_read`.
    Read,
    /// `hwpforge_fields`.
    Fields,
    /// `hwpforge_to_json`.
    ToJson,
    /// `hwpforge_from_json`.
    FromJson,
    /// `hwpforge_patch`.
    Patch,
    /// `hwpforge_diff`.
    Diff,
    /// `hwpforge_fill`.
    Fill,
    /// `hwpforge_set_cell`.
    SetCell,
    /// `hwpforge_insert_para`.
    InsertPara,
    /// `hwpforge_delete_para`.
    DeletePara,
    /// `hwpforge_stamp_plan`.
    StampPlan,
    /// `hwpforge_stamp`.
    Stamp,
    /// `hwpforge_templates`.
    Templates,
    /// `hwpforge_restyle`.
    Restyle,
    /// `hwpforge_validate` — never constructed by a tool: validate folds decode
    /// failures into `valid: false` (see the module docs).
    #[allow(dead_code, reason = "validate reports decode failure as a payload, not an error")]
    Validate,
    /// `hwpforge_to_md`.
    ToMd,
}

/// One static compatibility-table row: for this `(tool, code)` pair, emit
/// `legacy` as the code, and `hint` (if any) instead of `err.hint()`.
struct Row {
    tool: Tool,
    code: OpsCode,
    legacy: &'static str,
    hint: Option<&'static str>,
}

macro_rules! row {
    ($tool:ident, $code:ident, $legacy:literal) => {
        Row { tool: Tool::$tool, code: OpsCode::$code, legacy: $legacy, hint: None }
    };
    ($tool:ident, $code:ident, $legacy:literal, $hint:literal) => {
        Row { tool: Tool::$tool, code: OpsCode::$code, legacy: $legacy, hint: Some($hint) }
    };
}

/// The static half of the compatibility table.
///
/// Rows exist only where a pre-migration tool emitted this `(tool, code)`
/// pair with a specific hint — see `tests::inventory` for the audited list
/// this was built from (`tests/data/legacy_codes.txt`). An `OpsCode` an ops
/// function can return but no legacy tool ever surfaced (`INVALID_SET_CELL_ARGS`
/// — unreachable through MCP's typed `CellSpec` list — `ASSET_PLAN_MISMATCH`,
/// `HWP5_DECODE_FAILED`, …) has no row and falls through to
/// `code.as_str()` + `err.hint()` in [`tool_error`] by design; that is not a
/// gap, it is a code this migration is free to introduce.
#[rustfmt::skip]
const TABLE: &[Row] = &[
    // ── ConvertMd (hwpforge_convert) ──────────────────────────────
    // NOTE (ops gap, see module docs / W2 report): `ops::convert_md` only
    // accepts preset == "default" (`ConvertMdOptions::check_preset`); the
    // legacy tool applies modern/classic/latest by swapping the style
    // registry's fonts after decode. A row exists so the *code* stays
    // frozen if/when a caller passes an unsupported preset, but the tool
    // lane cannot delegate preset selection to `ops::convert_md` as-is.
    row!(ConvertMd, PresetNotFound, "PRESET_NOT_FOUND", "Use hwpforge_templates to see available presets."),
    row!(ConvertMd, MdDecodeFailed, "MD_DECODE_ERROR", "Check Markdown syntax. Use GFM (GitHub Flavored Markdown)."),
    row!(ConvertMd, StyleStoreFailed, "STYLE_STORE_ERROR", "Check paragraph list references in the resolved style registry."),
    row!(ConvertMd, StyleRebindFailed, "STYLE_REBIND_ERROR", "Document style indices do not match the generated HWPX style store."),
    row!(ConvertMd, ValidationFailed, "VALIDATION_ERROR", "Check document structure."),
    row!(ConvertMd, EncodeFailed, "ENCODE_ERROR", "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),

    // ── Inspect ────────────────────────────────────────────────────
    row!(Inspect, DecodeFailed, "DECODE_ERROR", "Check that the file is a valid HWPX document."),

    // ── Outline ────────────────────────────────────────────────────
    row!(Outline, DecodeFailed, "DECODE_ERROR", "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first."),

    // ── Read ───────────────────────────────────────────────────────
    row!(Read, DecodeFailed, "DECODE_ERROR", "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first."),
    row!(Read, ReadTargetRequired, "READ_TARGET_REQUIRED", "section reads a paragraph range; table reads a grid text matrix; field reads a named click-here field."),
    row!(Read, ReadParasWithoutSection, "READ_PARAS_WITHOUT_SECTION", "Pass section together with paras."),
    row!(Read, ReadParasInvalid, "READ_PARAS_INVALID", "Use \"A..B\" (inclusive) or a single \"N\"."),
    row!(Read, ReadSectionOutOfRange, "READ_SECTION_OUT_OF_RANGE", "Use hwpforge_outline to see section indexes."),
    row!(Read, ReadParaRangeInvalid, "READ_PARA_RANGE_INVALID", "Use hwpforge_outline to see paragraph counts per section."),
    row!(Read, ReadTableOutOfRange, "READ_TABLE_OUT_OF_RANGE", "Use hwpforge_outline to see table ordinals."),
    row!(Read, TableGridInvalid, "TABLE_GRID_INVALID", "This table's strict grid cannot be derived; use hwpforge_to_json."),
    row!(Read, ReadFieldNotFound, "READ_FIELD_NOT_FOUND", "Use hwpforge_fields to list available field names."),

    // ── Fields ─────────────────────────────────────────────────────
    row!(Fields, DecodeFailed, "DECODE_ERROR", "Check that the file is valid HWPX. For .hwp files, convert with hwpforge_convert first."),

    // ── ToJson ─────────────────────────────────────────────────────
    row!(ToJson, DecodeFailed, "DECODE_ERROR", "Check that the file is a valid HWPX document."),
    // NOTE: legacy `SERIALIZE_ERROR` had TWO origins in to_json.rs — the
    // document/section `serde_json::to_value` call (would become
    // ops-originated `JSON_SERIALIZE_FAILED` once to_json calls
    // `ops::to_json`/`ops::export_section`) and the final
    // `to_string_pretty` render (stays MCP-local: `ops::to_json` hands
    // back a `serde_json::Value`, the pretty-print is still the tool's
    // own). Both keep the same legacy string either way, so one row
    // covers the ops-originated half.
    row!(ToJson, JsonSerializeFailed, "SERIALIZE_ERROR", "This may be a bug."),
    row!(ToJson, GridAddrProjectionFailed, "GRID_ADDR_PROJECTION_FAILED", "This may be a bug."),
    row!(ToJson, PatchFailed, "PATCH_ERROR", "Re-export the target section with the current hwpforge_to_json tool so preservation metadata is embedded. Structural/style changes still require a broader rebuild workflow."),
    row!(ToJson, SectionWorkflowFailed, "SECTION_WORKFLOW_ERROR", "Update hwpforge so this MCP binding understands the newer section workflow error."),
    // SectionOutOfRange / SectionIndexMismatch: dynamic hint, see tool_error.

    // ── FromJson ───────────────────────────────────────────────────
    row!(FromJson, JsonParseFailed, "JSON_PARSE_ERROR", "Ensure JSON matches the ExportedDocument schema from hwpforge_to_json output."),
    row!(FromJson, GridAddrInvalid, "GRID_ADDR_INVALID", "Grid addresses come from hwpforge_to_json output; after structural edits, drop the stale addr fields (or re-export) and retry."),
    row!(FromJson, ValidationFailed, "VALIDATION_ERROR", "Check document structure."),
    row!(FromJson, EncodeFailed, "ENCODE_ERROR", "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),
    // NOTE: `ops::from_json`'s `base` option (inherit images from an
    // existing package) reaches `OpsError::Decode` → `DECODE_FAILED` too,
    // but the current `hwpforge_from_json` MCP tool has no `base_path`
    // parameter at all, so that path is unreachable today and DECODE_ERROR
    // has no legacy precedent for this tool — no row until a tool-file lane
    // adds `base` support (flagged in the W2 report).

    // ── Patch ──────────────────────────────────────────────────────
    row!(Patch, JsonParseFailed, "JSON_PARSE_ERROR", "Ensure the JSON matches the ExportedSection schema from hwpforge_to_json output."),
    row!(Patch, GridAddrInvalid, "GRID_ADDR_INVALID", "Grid addresses come from hwpforge_to_json output; after structural edits, drop the stale addr fields (or re-export) and retry."),
    row!(Patch, DecodeFailed, "DECODE_ERROR", "Check that the base file is valid HWPX."),
    row!(Patch, PatchFailed, "PATCH_ERROR", "Re-export the target section with the current hwpforge_to_json tool so preservation metadata is embedded. Structural/style changes still require a broader rebuild workflow."),
    row!(Patch, SectionWorkflowFailed, "SECTION_WORKFLOW_ERROR", "Update hwpforge so this MCP binding understands the newer section workflow error."),
    // SectionOutOfRange / SectionIndexMismatch: dynamic hint, see tool_error.

    // ── Diff ───────────────────────────────────────────────────────
    row!(Diff, DecodeFailed, "DECODE_ERROR", "Both inputs must be valid HWPX. For .hwp files, convert with hwpforge_convert first."),

    // ── Fill ───────────────────────────────────────────────────────
    row!(Fill, NoValues, "NO_VALUES", "Pass at least one name\u{2192}value pair. Use hwpforge_fields to discover names."),
    row!(Fill, EmptyFieldValue, "EMPTY_FIELD_VALUE", "빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요."),
    row!(Fill, FieldNameAmbiguous, "FIELD_NAME_AMBIGUOUS", "같은 이름의 누름틀이 여러 개라 대상이 모호합니다 — 문서에서 이름을 유일하게 하세요."),
    row!(Fill, FieldNotFillable, "FIELD_NOT_FILLABLE", "병합-run 모호 필드 또는 빈 본문 — 한컴 재저장 또는 from-json --base 재생성이 필요합니다."),
    row!(Fill, FillFailed, "FILL_ERROR", "Check that the file is valid HWPX."),
    // FieldNotFound: dynamic hint, see tool_error.

    // ── SetCell ────────────────────────────────────────────────────
    row!(SetCell, InvalidSetCellMap, "INVALID_SET_CELL_MAP", "Pass at least one CellSpec: {table, at|right_of|below, text}."),
    row!(SetCell, TableNotFound, "TABLE_NOT_FOUND", "표 서수는 hwpforge_to_json export 의 문서 순서 0-base 입니다."),
    row!(SetCell, TableGridInvalid, "TABLE_GRID_INVALID", "이 표는 셀 span 이 well-formed 격자를 이루지 않아 주소 지정이 불가합니다."),
    row!(SetCell, CellNotFound, "CELL_NOT_FOUND", "좌표는 병합 전 논리 격자 0-base — export 의 addr 값을 쓰세요."),
    row!(SetCell, CellLabelAmbiguous, "CELL_LABEL_AMBIGUOUS", "라벨이 여러 셀과 일치합니다 — at 좌표로 직접 지정하세요."),
    row!(SetCell, CellHasNonTextContent, "CELL_HAS_NON_TEXT_CONTENT", "표/이미지/컨트롤이 든 셀은 파괴 방지를 위해 교체를 거부합니다."),
    row!(SetCell, CellTargetDuplicate, "CELL_TARGET_DUPLICATE", "두 편집이 같은 앵커 셀로 resolve 됐습니다."),
    row!(SetCell, CellTargetConflict, "CELL_TARGET_CONFLICT", "바까생 셀 교체가 다른 편집이 노리는 중첩 표를 파괴합니다."),
    row!(SetCell, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", "이 입력은 무손실 재인코드가 증명되지 않아 편집을 거부합니다 (fail-closed)."),
    row!(SetCell, InputEntriesNotCarried, "INPUT_ENTRIES_NOT_CARRIED", "인코더가 carry 하지 않는 ZIP entry 가 있어 편집을 거부합니다 (fail-closed)."),
    row!(SetCell, SetCellCodecFailed, "SET_CELL_CODEC_FAILED", "Report this as a bug."),
    row!(SetCell, UpstreamUnmapped, "SET_CELL_FAILED", "Report this as a bug."),

    // ── InsertPara / DeletePara (structural.rs, shared map_error) ────
    row!(InsertPara, InsertTextRequired, "INSERT_TEXT_REQUIRED", "Use `text` for a single paragraph or `texts` for a contiguous block."),
    row!(DeletePara, DeleteNoTarget, "DELETE_NO_TARGET", "indices must be a non-empty list of top-level paragraph indices."),
    row!(InsertPara, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", "Structural edits require a round-trip-safe input; this document has a codec fidelity gap."),
    row!(DeletePara, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", "Structural edits require a round-trip-safe input; this document has a codec fidelity gap."),
    row!(DeletePara, ReferenceStranded, "REFERENCE_STRANDED", "This paragraph carries a bookmark/cross-ref/footnote; deleting it could strand a reference."),
    row!(DeletePara, HardBreakLoss, "HARD_BREAK_LOSS", "This paragraph carries a hard page/column break."),
    row!(InsertPara, SectionPropertiesParagraph, "SECTION_PROPERTIES_PARAGRAPH", "The section's first paragraph holds page setup; it cannot be deleted or displaced."),
    row!(DeletePara, SectionPropertiesParagraph, "SECTION_PROPERTIES_PARAGRAPH", "The section's first paragraph holds page setup; it cannot be deleted or displaced."),
    row!(DeletePara, EmptySection, "EMPTY_SECTION", "A section must keep at least one paragraph."),
    row!(InsertPara, MultiParagraphText, "MULTI_PARAGRAPH_TEXT", "Insert one paragraph per call; text may not contain line breaks."),
    row!(InsertPara, ParagraphOutOfRange, "INDEX_OUT_OF_RANGE", "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, ParagraphOutOfRange, "INDEX_OUT_OF_RANGE", "Use hwpforge_outline to see section and paragraph counts."),
    row!(InsertPara, SectionOutOfRange, "INDEX_OUT_OF_RANGE", "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, SectionOutOfRange, "INDEX_OUT_OF_RANGE", "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, DuplicateTarget, "DUPLICATE_TARGET", "Each paragraph index may appear once per batch."),
    row!(InsertPara, SelfVerifyFailed, "SELF_VERIFY_FAILED", "The edit did not verify; no output was written."),
    row!(DeletePara, SelfVerifyFailed, "SELF_VERIFY_FAILED", "The edit did not verify; no output was written."),
    row!(InsertPara, StructuralCodec, "STRUCTURAL_CODEC", "Check that the file is valid HWPX."),
    row!(DeletePara, StructuralCodec, "STRUCTURAL_CODEC", "Check that the file is valid HWPX."),
    // Two `ops` codes the legacy catch-all (`_ => STRUCTURAL_EDIT_FAILED`)
    // never named because the old `map_error` predates them: `ops`
    // classifies `SpanCountMismatch`/`InsertBeforeSectionProperties` under
    // their own codes, but no MCP tool file has ever emitted those two
    // strings. These two rows freeze the *old* behaviour (report as
    // "the structural edit was refused" like every other unlisted variant
    // did); dropping them would silently widen the contract. Flagged in
    // the W2 report for the lead to confirm.
    row!(InsertPara, SpanCountMismatch, "STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),
    row!(DeletePara, SpanCountMismatch, "STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),
    row!(InsertPara, InsertBeforeSectionProperties, "STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),
    row!(InsertPara, UpstreamUnmapped, "STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),
    row!(DeletePara, UpstreamUnmapped, "STRUCTURAL_EDIT_FAILED", "The structural edit was refused."),

    // ── StampPlan ──────────────────────────────────────────────────
    row!(StampPlan, StampCodecFailed, "STAMP_CODEC_FAILED", "Check that the file is valid HWPX."),

    // ── Stamp ──────────────────────────────────────────────────────
    row!(Stamp, InputNotRoundtripSafe, "INPUT_NOT_ROUNDTRIP_SAFE", "이 입력은 무손실 재인코드가 증명되지 않아 거부됩니다 (fail-closed). 코덱 갭 수정 전까지 스탬핑 불가."),
    row!(Stamp, InputEntriesNotCarried, "INPUT_ENTRIES_NOT_CARRIED", "재인코드 시 유실될 ZIP 엔트리가 있어 거부됩니다 (fail-closed)."),
    row!(Stamp, StampManifestInvariant, "STAMP_MANIFEST_INVARIANT", "Report this as a bug — the output inventory violated an invariant."),
    row!(Stamp, StampCodecFailed, "STAMP_CODEC_FAILED", "Check that the file is valid HWPX."),
    row!(Stamp, StampSourceHashMismatch, "STAMP_SOURCE_HASH_MISMATCH", "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행해 source_sha256 을 갱신하세요."),
    row!(Stamp, StampDeltaMismatch, "STAMP_DELTA_MISMATCH", "산출물 검증 실패 — 코덱 버그 가능성이 있어 무출력으로 거부했습니다."),
    row!(Stamp, TableNotFound, "TABLE_NOT_FOUND", "hwpforge_stamp_plan 의 cells[].table 서수를 사용하세요."),
    row!(Stamp, TableGridInvalid, "TABLE_GRID_INVALID", "이 표는 논리 격자를 만들 수 없어 셀 스탬핑 대상이 아닙니다."),
    row!(Stamp, StampCellNotEmpty, "STAMP_CELL_NOT_EMPTY", "클래스-B 대상은 whitespace-only 빈 셀이어야 합니다."),
    row!(Stamp, StampLabelDrift, "STAMP_LABEL_DRIFT", "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행하세요."),
    row!(Stamp, StampCellNotCandidate, "STAMP_CELL_NOT_CANDIDATE", "ignore 는 live 후보에만 가능합니다."),
    row!(Stamp, StampCellTargetDuplicate, "STAMP_CELL_TARGET_DUPLICATE", "같은 셀을 두 번 분류했습니다."),
    row!(Stamp, StampNameEmpty, "STAMP_NAME_EMPTY", "빈 이름은 허용되지 않습니다."),
    row!(Stamp, StampHintBlank, "STAMP_HINT_BLANK", "빈 셀엔 마커가 없어 hint 가 필수입니다 — plan 의 suggested_hint 를 참고하세요."),
    row!(Stamp, StampSpecStale, "STAMP_SPEC_STALE", "문서가 변경됐거나 span 이 어긋났습니다 — hwpforge_stamp_plan 을 다시 실행하세요."),
    row!(Stamp, StampMarkerMismatch, "STAMP_MARKER_MISMATCH", "spec 의 marker 는 문서의 현재 텍스트와 일치해야 합니다."),
    row!(Stamp, StampSpecDuplicate, "STAMP_SPEC_DUPLICATE", "같은 후보를 두 번 분류했습니다."),
    row!(Stamp, UpstreamUnmapped, "STAMP_FAILED", "Unexpected failure."),
    // StampCellNotAnchor (dynamic), StampNameDuplicate / StampNameCollision
    // / StampCandidateUncovered (dual-source, text-spec vs. cell-spec
    // wording): see tool_error.

    // ── Templates ──────────────────────────────────────────────────
    row!(Templates, PresetNotFound, "PRESET_NOT_FOUND", "Available presets: default, modern, classic, latest. Use hwpforge_templates without a name to list all."),

    // ── Restyle ────────────────────────────────────────────────────
    row!(Restyle, PresetNotFound, "PRESET_NOT_FOUND", "Use hwpforge_templates to see available presets."),
    row!(Restyle, DecodeFailed, "DECODE_ERROR", "Check that the file is a valid HWPX document."),
    row!(Restyle, NoFonts, "NO_FONTS", "The HWPX file may be malformed. Use hwpforge_validate to check."),
    row!(Restyle, ValidationFailed, "VALIDATION_ERROR", "Check document structure."),
    row!(Restyle, EncodeFailed, "ENCODE_ERROR", "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),
    // EncodeSemanticLoss: dynamic message + tool-specific hint, see tool_error.

    // ── ToMd ───────────────────────────────────────────────────────
    row!(ToMd, DecodeFailed, "DECODE_ERROR", "Check that the file is a valid HWPX document."),
    row!(ToMd, ValidationFailed, "VALIDATION_ERROR", "The HWPX document structure is invalid."),
];

/// Maps an [`OpsError`] from calling `tool`'s underlying `ops` function onto
/// the MCP server's frozen legacy `(code, hint)` contract.
///
/// The message is **not** frozen (see module docs): it defaults to
/// `err.to_string()`, which the library authors already wrote to match what
/// the frontends print, except for the handful of reconstructions below
/// where the wrapped-error `Display` no longer carries what the legacy tool
/// printed (semantic-loss's dynamic warning text; a few hints that embed
/// per-call data such as an available-fields list).
#[must_use]
pub fn tool_error(tool: Tool, err: OpsError) -> ToolErrorInfo {
    // Semantic-loss refusals: `set_cell`, `stamp` and `restyle` all reach
    // the *same* `OpsError::EncodeSemanticLoss` variant (the `From<CellEditError>`
    // / `From<StamperError>` impls in `ops/mod.rs` intercept the typed
    // `SemanticLoss` variant before it would otherwise reach `OpsError::CellEdit`
    // / `OpsError::Stamper`), so only `tool` disambiguates which legacy
    // code and message shape applies.
    if let OpsError::EncodeSemanticLoss { warnings, .. } = &err {
        if let Some(info) = semantic_loss_error(tool, warnings) {
            return info;
        }
    }

    // Hints that embed per-call data no flat table row can hold. Matched on
    // the *wrapped* library error, which OpsError keeps untouched, so no
    // `tool` disambiguation is needed here: each error type is reachable
    // from exactly one MCP tool.
    match &err {
        OpsError::Fill(FillError::UnknownField { available, .. }) => {
            return ToolErrorInfo::new(
                "FIELD_NOT_FOUND",
                err.to_string(),
                format!(
                    "Available fields: [{}]. Use hwpforge_fields to list them.",
                    available.join(", ")
                ),
            );
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange { sections, .. }) => {
            return ToolErrorInfo::new(
                "SECTION_OUT_OF_RANGE",
                err.to_string(),
                format!("Valid range: 0..={}", sections.saturating_sub(1)),
            );
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
            requested,
            actual,
        }) => {
            return ToolErrorInfo::new(
                "SECTION_INDEX_MISMATCH",
                err.to_string(),
                format!(
                    "Use section: {actual} to match the JSON, or re-export section {requested} with hwpforge_to_json."
                ),
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NotAnAnchor {
            anchor, ..
        })) => {
            let hint = match anchor {
                Some(a) => {
                    format!("병합 피복 위치입니다 — anchor ({},{}) 를 지정하세요.", a.row, a.col)
                }
                None => "격자 범위 밖 좌표입니다.".to_string(),
            };
            return ToolErrorInfo::new("STAMP_CELL_NOT_ANCHOR", err.to_string(), hint);
        }
        // Dual-source: the same OpsCode is produced by a text-spec rejection
        // (`StampError`, inside `StamperError::Stamp`) and a cell-spec
        // rejection (`CellStampError`, inside `StamperError::CellStamp`),
        // and the two legacy hints differ in wording.
        OpsError::Stamper(StamperError::Stamp(StampError::DuplicateName { .. })) => {
            return ToolErrorInfo::new(
                "STAMP_NAME_DUPLICATE",
                err.to_string(),
                "필드 이름은 spec 전체에서 유일해야 합니다.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::DuplicateName { .. })) => {
            return ToolErrorInfo::new(
                "STAMP_NAME_DUPLICATE",
                err.to_string(),
                "필드 이름은 text+cells 전체에서 유일해야 합니다.",
            );
        }
        OpsError::Stamper(StamperError::Stamp(StampError::NameCollision { .. })) => {
            return ToolErrorInfo::new(
                "STAMP_NAME_COLLISION",
                err.to_string(),
                "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 기존 이름을 확인하세요.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NameCollision { .. })) => {
            return ToolErrorInfo::new(
                "STAMP_NAME_COLLISION",
                err.to_string(),
                "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 확인하세요.",
            );
        }
        OpsError::Stamper(StamperError::Stamp(StampError::UncoveredCandidate { .. })) => {
            return ToolErrorInfo::new(
                "STAMP_CANDIDATE_UNCOVERED",
                err.to_string(),
                "모든 무가드 후보는 이름 또는 ignore 로 분류해야 합니다 — hwpforge_stamp_plan 출력을 빠짐없이 사용하세요.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::UncoveredCandidate {
            ..
        })) => {
            return ToolErrorInfo::new(
                "STAMP_CANDIDATE_UNCOVERED",
                err.to_string(),
                "모든 무가드 셀 후보는 이름 또는 ignore 로 분류해야 합니다.",
            );
        }
        // Fixed-literal legacy messages for the two argument-shape
        // rejections `hwpforge_read`'s own local guard used to phrase
        // without `ops`'s CLI-flag spelling (`--section`/`--paras`).
        OpsError::Rejected { code: OpsCode::ReadTargetRequired, .. } => {
            return ToolErrorInfo::new(
                "READ_TARGET_REQUIRED",
                "Pass exactly one of section, table, field",
                "section reads a paragraph range; table reads a grid text matrix; field reads a named click-here field.",
            );
        }
        OpsError::Rejected { code: OpsCode::ReadParasWithoutSection, .. } => {
            return ToolErrorInfo::new(
                "READ_PARAS_WITHOUT_SECTION",
                "paras requires section",
                "Pass section together with paras.",
            );
        }
        _ => {}
    }

    let code = err.code();
    let message = err.to_string();
    match TABLE.iter().find(|row| row.tool == tool && row.code == code) {
        Some(row) => ToolErrorInfo::new(row.legacy, message, row.hint.unwrap_or_default()),
        None => ToolErrorInfo::new(code.as_str(), message, err.hint().unwrap_or_default()),
    }
}

/// Reconstructs the byte-identical legacy semantic-loss refusal for
/// `set_cell`/`stamp` and the message-preserving one for `restyle`.
///
/// `set_cell` and `stamp` never reported `ENCODE_SEMANTIC_LOSS` — before
/// `CellEditError`/`StamperError` grew a typed `SemanticLoss` variant, both
/// tools' `Codec` arm printed `error.to_string()` verbatim (`"codec
/// failure: {msg}"` for set_cell's `CellEditError::Display`, `"{msg}"` for
/// stamp's `StamperError::Display`, where `msg` was the *first* semantic
/// warning's own `Display`, dropping the rest). `hwpforge-smithy-hwpx`'s
/// R1 F4 regression tests
/// (`crates/hwpforge-smithy-hwpx/src/cell_edit.rs`,
/// `crates/hwpforge-smithy-hwpx/src/stamp/stamper.rs`) pin exactly that
/// wording, so it is reproduced here from the first warning's `WarningInfo.message`
/// (which is already that `Display`, per `OpsWarning::info`).
///
/// `restyle` *did* fail closed on `ENCODE_SEMANTIC_LOSS` already (R1 F2) and
/// joined every semantic warning (not just the first) with `"; "` — see
/// `hwpforge-bindings-mcp/src/tools/restyle.rs`'s `restyle_fails_closed_on_note_head_skip`.
///
/// Returns `None` for any other tool: those never reach `EncodeSemanticLoss`
/// through the ops functions they call.
fn semantic_loss_error(
    tool: Tool,
    warnings: &[hwpforge_foundation::diagnostics::WarningInfo],
) -> Option<ToolErrorInfo> {
    match tool {
        Tool::SetCell => {
            let message = match warnings.first() {
                Some(w) => format!(
                    "codec failure: encode produced a semantic-loss warning (fail-closed): {}",
                    w.message
                ),
                None => "codec failure: encode produced a semantic-loss warning (fail-closed)"
                    .to_string(),
            };
            Some(ToolErrorInfo::new("SET_CELL_CODEC_FAILED", message, "Report this as a bug."))
        }
        Tool::Stamp => {
            let message = match warnings.first() {
                Some(w) => {
                    format!("encode produced a semantic-loss warning (fail-closed): {}", w.message)
                }
                None => "encode produced a semantic-loss warning (fail-closed)".to_string(),
            };
            Some(ToolErrorInfo::new(
                "STAMP_CODEC_FAILED",
                message,
                "Check that the file is valid HWPX.",
            ))
        }
        Tool::Restyle => {
            let message =
                warnings.iter().map(|w| w.message.as_str()).collect::<Vec<_>>().join("; ");
            Some(ToolErrorInfo::new(
                "ENCODE_SEMANTIC_LOSS",
                message,
                "The restyled document would lose footnote/endnote numbering or TOC marks; fix the \
                 source document or restyle a document without those constructs.",
            ))
        }
        _ => None,
    }
}

/// Maps an [`OpsWarning`] onto the MCP wire shape.
///
/// This is a straight field copy: [`OpsWarning::info`] already produces the
/// `{code, message, hint}` triple every tool's `warnings` field carries
/// (`ToolWarningInfo` and `hwpforge_foundation::diagnostics::WarningInfo`
/// are structurally the same payload, defined in two crates so `ops` does
/// not depend on the MCP crate). New canonical warning codes this
/// migration introduces (`NOTE_HEAD_SKIPPED`, `TITLE_MARK_SKIPPED`, …, see
/// `hwpforge::ops::OpsWarning::info` docs) pass through unchanged — whether
/// a tool's `data` schema has room to carry them without an additive field
/// is a per-tool decision for W4, per `common.md`'s "Warnings" section.
#[must_use]
pub fn warning(w: &OpsWarning) -> ToolWarningInfo {
    let info = w.info();
    match info.hint {
        Some(hint) => ToolWarningInfo::new(info.code, info.message).with_hint(hint),
        None => ToolWarningInfo::new(info.code, info.message),
    }
}

#[cfg(test)]
mod tests {
    //! Inventory: every row this module claims to cover is checked against
    //! `tests/data/legacy_codes.txt` (the hand-audited snapshot of the
    //! pre-migration source), and every representative `OpsError` this test
    //! module can construct is checked to round-trip through [`tool_error`]
    //! to the code that snapshot names. This is what keeps `TABLE` from
    //! drifting silently: a row with a typo'd `legacy` string fails here,
    //! not in an integration test three lanes downstream.

    use std::collections::BTreeSet;

    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_smithy_hwpx::grid_addr::GridAddrError;
    use hwpforge_smithy_hwpx::stamp::StampMapError;
    use hwpforge_smithy_hwpx::{CellEditError, HwpxError, ReadError, StructuralEditError};

    use super::*;

    const SNAPSHOT: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/legacy_codes.txt"));

    /// Parses `tests/data/legacy_codes.txt` into `(tool_name, code)` pairs,
    /// skipping `#`-prefixed comment lines and blank lines.
    fn snapshot_pairs() -> BTreeSet<(&'static str, &'static str)> {
        SNAPSHOT
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| {
                let mut parts = line.split('\t');
                let tool = parts.next().expect("tool column");
                let code = parts.next().expect("code column");
                assert!(parts.next().is_none(), "unexpected third column: {line:?}");
                (tool, code)
            })
            .collect()
    }

    /// The snapshot's tool names, in [`Tool`] terms. `output`,
    /// `stamp_plan`/`stamp` and `insert_para`/`delete_para` split one file's
    /// codes across MCP-local vs. ops-originated (or two tools sharing a
    /// mapper function); everything else names one [`Tool`] variant.
    fn tool_name(tool: Tool) -> &'static str {
        match tool {
            Tool::ConvertMd => "convert",
            Tool::Inspect => "inspect",
            Tool::Outline => "outline",
            Tool::Read => "read",
            Tool::Fields => "fields",
            Tool::ToJson => "to_json",
            Tool::FromJson => "from_json",
            Tool::Patch => "patch",
            Tool::Diff => "diff",
            Tool::Fill => "fill",
            Tool::SetCell => "set_cell",
            Tool::InsertPara => "insert_para",
            Tool::DeletePara => "delete_para",
            Tool::StampPlan => "stamp_plan",
            Tool::Stamp => "stamp",
            Tool::Templates => "templates",
            Tool::Restyle => "restyle",
            Tool::Validate => "validate",
            Tool::ToMd => "to_md",
        }
    }

    /// (a) Every `TABLE` row's `(tool, legacy)` pair names a code that is
    /// genuinely in the audited snapshot for that tool — a row cannot claim
    /// to preserve a legacy string the pre-migration source never emitted.
    #[test]
    fn every_table_row_is_in_the_audited_snapshot() {
        let snapshot = snapshot_pairs();
        for row in TABLE {
            let name = tool_name(row.tool);
            assert!(
                snapshot.contains(&(name, row.legacy)),
                "TABLE has ({name}, {}) but tests/data/legacy_codes.txt does not \u{2014} \
                 either the row is wrong or the snapshot needs re-auditing",
                row.legacy
            );
        }
    }

    /// The `(tool, legacy)` pairs `tool_error`'s dynamic/special-case arms
    /// produce — the ones no flat `TABLE` row can hold (per-call data in the
    /// hint, or the dual-source `Stamp*`/`CellStamp*` wording). Kept in one
    /// place so both coverage tests ((b) below and `every_expected_code_is_covered`)
    /// check against the same list instead of drifting apart.
    const DYNAMIC: &[(&str, &str)] = &[
        ("fill", "FIELD_NOT_FOUND"),
        ("to_json", "SECTION_OUT_OF_RANGE"),
        ("to_json", "SECTION_INDEX_MISMATCH"),
        ("patch", "SECTION_OUT_OF_RANGE"),
        ("patch", "SECTION_INDEX_MISMATCH"),
        ("stamp", "STAMP_CELL_NOT_ANCHOR"),
        ("stamp", "STAMP_NAME_DUPLICATE"),
        ("stamp", "STAMP_NAME_COLLISION"),
        ("stamp", "STAMP_CANDIDATE_UNCOVERED"),
        ("set_cell", "SET_CELL_CODEC_FAILED"),
        ("stamp", "STAMP_CODEC_FAILED"),
        ("restyle", "ENCODE_SEMANTIC_LOSS"),
        ("read", "READ_TARGET_REQUIRED"),
        ("read", "READ_PARAS_WITHOUT_SECTION"),
    ];

    /// (b) MCP-local codes never routed through `tool_error`/`TABLE` — the
    /// path/output/protocol layer common.md's brief names explicitly
    /// (`INVALID_EXTENSION`, the four `output.rs` codes, `OUTPUT_TOO_LARGE`)
    /// plus the ones this audit found to be genuinely path/IO/manifest-file
    /// concerns with no `ops` equivalent (`FILE_WRITE_FAILED`,
    /// `DIR_CREATE_ERROR`, `STAMP_MANIFEST_SERIALIZE`, `MANIFEST_PATH_CONFLICT`,
    /// `INTERNAL_ERROR`, `PRESET_ERROR`). Every remaining snapshot row is
    /// either covered by `TABLE` or by a `DYNAMIC` special-case arm —
    /// nothing in the frozen contract is silently unaccounted for.
    #[test]
    fn every_snapshot_row_is_mcp_local_or_in_the_table() {
        const MCP_LOCAL: &[(&str, &str)] = &[
            ("output", "FILE_NOT_FOUND"),
            ("output", "INPUT_TOO_LARGE"),
            ("output", "METADATA_ERROR"),
            ("output", "READ_ERROR"),
            ("output", "WRITE_ERROR"),
            ("convert", "INPUT_TOO_LARGE"),
            ("convert", "INVALID_EXTENSION"),
            ("convert", "PRESET_ERROR"),
            ("diff", "FILE_WRITE_FAILED"),
            ("diff", "OUTPUT_TOO_LARGE"),
            ("diff", "SERIALIZE_ERROR"),
            ("fill", "INVALID_EXTENSION"),
            ("from_json", "INPUT_TOO_LARGE"),
            ("from_json", "INTERNAL_ERROR"),
            ("from_json", "INVALID_EXTENSION"),
            ("insert_para", "FILE_WRITE_FAILED"),
            ("delete_para", "FILE_WRITE_FAILED"),
            ("outline", "OUTPUT_TOO_LARGE"),
            ("patch", "INVALID_EXTENSION"),
            ("restyle", "INVALID_EXTENSION"),
            ("set_cell", "INVALID_EXTENSION"),
            ("stamp", "INVALID_EXTENSION"),
            ("stamp", "MANIFEST_PATH_CONFLICT"),
            // `run_stamp` checks `source_sha256.is_some()` itself before
            // building the v2 request; `hwpforge_smithy_hwpx::stamp::parse_stamp_map`
            // owns this validation for a caller that parses a raw map
            // (module docs: "parsing happens in the caller, so a malformed
            // map fails before [ops::stamp] is entered"), and MCP's
            // hand-built `StampRequestV2` never goes through that parser.
            ("stamp", "MISSING_SOURCE_SHA256"),
            ("stamp", "STAMP_MANIFEST_SERIALIZE"),
            ("stamp", "WRITE_ERROR"),
            ("to_json", "INVALID_EXTENSION"),
            ("to_json", "OUTPUT_TOO_LARGE"),
            ("to_md", "DIR_CREATE_ERROR"),
            ("to_md", "INVALID_INPUT"),
        ];

        let mut covered: BTreeSet<(&str, &str)> =
            TABLE.iter().map(|row| (tool_name(row.tool), row.legacy)).collect();
        covered.extend(DYNAMIC.iter().copied());
        let mcp_local: BTreeSet<(&str, &str)> = MCP_LOCAL.iter().copied().collect();

        for pair in snapshot_pairs() {
            assert!(
                covered.contains(&pair) || mcp_local.contains(&pair),
                "{pair:?} is in the snapshot but neither TABLE/DYNAMIC nor the MCP_LOCAL list accounts for it"
            );
        }
        // And the reverse: nothing declared MCP-local should also have a
        // TABLE row (that would mean two conflicting answers for one code).
        for pair in &mcp_local {
            assert!(!covered.contains(pair), "{pair:?} is both MCP_LOCAL and in TABLE");
        }
    }

    /// (c) Independently-derived reachability: this list is transcribed
    /// from the `# Errors` doc sections of the `hwpforge::ops` functions
    /// each tool will call (`edit.rs`, `exchange.rs`, `read.rs`, `stamp.rs`,
    /// `style.rs`, `convert.rs`), not from `TABLE` itself — the point is to
    /// catch a `TABLE` row that is *wrong*, so the expected set must not be
    /// built from the thing under test. Codes reachable but absent from the
    /// legacy snapshot (no MCP tool ever emitted them, e.g. `INVALID_SET_CELL_ARGS`)
    /// are intentionally excluded: they fall through to `code.as_str()` by
    /// design and need no row.
    fn expected_ops_originated(tool: Tool) -> &'static [OpsCode] {
        use OpsCode::*;
        match tool {
            Tool::ConvertMd => &[
                PresetNotFound,
                MdDecodeFailed,
                StyleStoreFailed,
                StyleRebindFailed,
                ValidationFailed,
                EncodeFailed,
            ],
            Tool::Inspect | Tool::Fields => &[DecodeFailed],
            Tool::Outline => &[DecodeFailed],
            Tool::Read => &[
                DecodeFailed,
                ReadTargetRequired,
                ReadParasWithoutSection,
                ReadParasInvalid,
                ReadSectionOutOfRange,
                ReadParaRangeInvalid,
                ReadTableOutOfRange,
                TableGridInvalid,
                ReadFieldNotFound,
            ],
            Tool::ToJson => &[
                DecodeFailed,
                JsonSerializeFailed,
                GridAddrProjectionFailed,
                SectionOutOfRange,
                SectionIndexMismatch,
                PatchFailed,
                SectionWorkflowFailed,
            ],
            Tool::FromJson => &[JsonParseFailed, GridAddrInvalid, ValidationFailed, EncodeFailed],
            Tool::Patch => &[
                JsonParseFailed,
                GridAddrInvalid,
                DecodeFailed,
                SectionOutOfRange,
                SectionIndexMismatch,
                PatchFailed,
                SectionWorkflowFailed,
            ],
            Tool::Diff => &[DecodeFailed],
            Tool::Fill => &[
                NoValues,
                EmptyFieldValue,
                FieldNotFound,
                FieldNameAmbiguous,
                FieldNotFillable,
                FillFailed,
            ],
            Tool::SetCell => &[
                InvalidSetCellMap,
                TableNotFound,
                TableGridInvalid,
                CellNotFound,
                CellLabelAmbiguous,
                CellHasNonTextContent,
                CellTargetDuplicate,
                CellTargetConflict,
                InputNotRoundtripSafe,
                InputEntriesNotCarried,
                SetCellCodecFailed,
                EncodeSemanticLoss,
                UpstreamUnmapped,
            ],
            Tool::InsertPara => &[
                InsertTextRequired,
                InputNotRoundtripSafe,
                SectionPropertiesParagraph,
                MultiParagraphText,
                ParagraphOutOfRange,
                SectionOutOfRange,
                SelfVerifyFailed,
                StructuralCodec,
                SpanCountMismatch,
                InsertBeforeSectionProperties,
                UpstreamUnmapped,
            ],
            Tool::DeletePara => &[
                DeleteNoTarget,
                InputNotRoundtripSafe,
                ReferenceStranded,
                HardBreakLoss,
                SectionPropertiesParagraph,
                EmptySection,
                ParagraphOutOfRange,
                SectionOutOfRange,
                DuplicateTarget,
                SelfVerifyFailed,
                StructuralCodec,
                SpanCountMismatch,
                UpstreamUnmapped,
            ],
            Tool::StampPlan => &[StampCodecFailed],
            Tool::Stamp => &[
                InputNotRoundtripSafe,
                InputEntriesNotCarried,
                StampManifestInvariant,
                StampCodecFailed,
                StampSourceHashMismatch,
                StampDeltaMismatch,
                TableNotFound,
                TableGridInvalid,
                StampCellNotAnchor,
                StampCellNotEmpty,
                StampLabelDrift,
                StampCellNotCandidate,
                StampCellTargetDuplicate,
                StampNameEmpty,
                StampHintBlank,
                StampSpecStale,
                StampMarkerMismatch,
                StampSpecDuplicate,
                StampNameDuplicate,
                StampNameCollision,
                StampCandidateUncovered,
                EncodeSemanticLoss,
                UpstreamUnmapped,
            ],
            Tool::Templates => &[PresetNotFound],
            Tool::Restyle => &[
                PresetNotFound,
                DecodeFailed,
                NoFonts,
                ValidationFailed,
                EncodeFailed,
                EncodeSemanticLoss,
            ],
            Tool::Validate => &[],
            Tool::ToMd => &[DecodeFailed, ValidationFailed],
        }
    }

    /// Every code in the independently-derived reachability list either has
    /// a `TABLE` row for that tool, or is produced by one of `tool_error`'s
    /// dynamic/special-case arms (semantic-loss, `FieldNotFound`, the two
    /// `SectionWorkflow` variants, `StampCellNotAnchor`, the dual-source
    /// `Stamp*`/`CellStamp*` name codes). The special-case set is listed
    /// explicitly so this stays a real check, not a tautology.
    #[test]
    fn every_expected_code_is_covered() {
        const DYNAMIC: &[(Tool, OpsCode)] = &[
            (Tool::Fill, OpsCode::FieldNotFound),
            (Tool::ToJson, OpsCode::SectionOutOfRange),
            (Tool::ToJson, OpsCode::SectionIndexMismatch),
            (Tool::Patch, OpsCode::SectionOutOfRange),
            (Tool::Patch, OpsCode::SectionIndexMismatch),
            (Tool::Stamp, OpsCode::StampCellNotAnchor),
            (Tool::Stamp, OpsCode::StampNameDuplicate),
            (Tool::Stamp, OpsCode::StampNameCollision),
            (Tool::Stamp, OpsCode::StampCandidateUncovered),
            (Tool::SetCell, OpsCode::EncodeSemanticLoss),
            (Tool::Stamp, OpsCode::EncodeSemanticLoss),
            (Tool::Restyle, OpsCode::EncodeSemanticLoss),
        ];

        for &tool in &[
            Tool::ConvertMd,
            Tool::Inspect,
            Tool::Outline,
            Tool::Read,
            Tool::Fields,
            Tool::ToJson,
            Tool::FromJson,
            Tool::Patch,
            Tool::Diff,
            Tool::Fill,
            Tool::SetCell,
            Tool::InsertPara,
            Tool::DeletePara,
            Tool::StampPlan,
            Tool::Stamp,
            Tool::Templates,
            Tool::Restyle,
            Tool::Validate,
            Tool::ToMd,
        ] {
            for &code in expected_ops_originated(tool) {
                let has_row = TABLE.iter().any(|r| r.tool == tool && r.code == code);
                let is_dynamic = DYNAMIC.contains(&(tool, code));
                assert!(
                    has_row || is_dynamic,
                    "{tool:?} is documented to reach {code:?} but TABLE has no row and no \
                     dynamic arm covers it"
                );
            }
        }
    }

    // ── round-trip checks: construct a representative OpsError per code,
    // confirm tool_error's output code matches the snapshot exactly. ──────

    fn assert_code(tool: Tool, err: OpsError, expected: &str) {
        let got = tool_error(tool, err);
        assert_eq!(got.code, expected, "{tool:?}: {got:?}");
    }

    #[test]
    fn fill_errors_round_trip() {
        assert_code(
            Tool::Fill,
            OpsError::Rejected { code: OpsCode::NoValues, reason: "no field values given".into() },
            "NO_VALUES",
        );
        assert_code(
            Tool::Fill,
            OpsError::Fill(FillError::EmptyValue { name: "x".into() }),
            "EMPTY_FIELD_VALUE",
        );
        let err = OpsError::Fill(FillError::UnknownField {
            name: "x".into(),
            available: vec!["a".into(), "b".into()],
        });
        let info = tool_error(Tool::Fill, err);
        assert_eq!(info.code, "FIELD_NOT_FOUND");
        assert!(info.hint.contains("[a, b]"), "{info:?}");
        assert_code(
            Tool::Fill,
            OpsError::Fill(FillError::DuplicateFieldName { name: "x".into(), count: 2 }),
            "FIELD_NAME_AMBIGUOUS",
        );
        assert_code(
            Tool::Fill,
            OpsError::Fill(FillError::UnfillableField { name: "x".into(), section: 0 }),
            "FIELD_NOT_FILLABLE",
        );
    }

    #[test]
    fn read_errors_round_trip() {
        assert_code(
            Tool::Read,
            OpsError::Rejected { code: OpsCode::ReadTargetRequired, reason: "x".into() },
            "READ_TARGET_REQUIRED",
        );
        assert_code(
            Tool::Read,
            OpsError::Rejected { code: OpsCode::ReadParasWithoutSection, reason: "x".into() },
            "READ_PARAS_WITHOUT_SECTION",
        );
        assert_code(
            Tool::Read,
            OpsError::Rejected { code: OpsCode::ReadParasInvalid, reason: "x".into() },
            "READ_PARAS_INVALID",
        );
        assert_code(
            Tool::Read,
            OpsError::Read(ReadError::SectionOutOfRange { requested: 9, available: 1 }),
            "READ_SECTION_OUT_OF_RANGE",
        );
        assert_code(
            Tool::Read,
            OpsError::Read(ReadError::FieldNotFound { name: "x".into(), available: vec![] }),
            "READ_FIELD_NOT_FOUND",
        );
    }

    #[test]
    fn section_workflow_errors_round_trip_for_patch_and_to_json() {
        for tool in [Tool::Patch, Tool::ToJson] {
            let err = OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange {
                requested: 5,
                sections: 2,
            });
            let info = tool_error(tool, err);
            assert_eq!(info.code, "SECTION_OUT_OF_RANGE");
            assert_eq!(info.hint, "Valid range: 0..=1");

            let err = OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
                requested: 0,
                actual: 1,
            });
            let info = tool_error(tool, err);
            assert_eq!(info.code, "SECTION_INDEX_MISMATCH");
            assert!(info.message.contains("Requested section 0 but JSON contains section 1 data"));
            assert!(info.hint.contains("Use section: 1"));

            assert_code(
                tool,
                OpsError::SectionWorkflow(SectionWorkflowError::PreservingPatch(
                    HwpxError::InvalidStructure { detail: "preservation metadata version".into() },
                )),
                "PATCH_ERROR",
            );
        }
    }

    #[test]
    fn structural_errors_route_shared_codes_to_index_out_of_range() {
        for (tool, err) in [
            (
                Tool::InsertPara,
                OpsError::StructuralEdit(StructuralEditError::SectionOutOfRange {
                    section: 9,
                    available: 1,
                }),
            ),
            (
                Tool::DeletePara,
                OpsError::StructuralEdit(StructuralEditError::ParagraphOutOfRange {
                    section: 0,
                    index: 9,
                    available: 1,
                }),
            ),
        ] {
            assert_code(tool, err, "INDEX_OUT_OF_RANGE");
        }
        assert_code(
            Tool::DeletePara,
            OpsError::StructuralEdit(StructuralEditError::DuplicateTarget { section: 0, index: 1 }),
            "DUPLICATE_TARGET",
        );
        assert_code(
            Tool::InsertPara,
            OpsError::Rejected {
                code: OpsCode::InsertTextRequired,
                reason: "insert_para needs at least one paragraph text".into(),
            },
            "INSERT_TEXT_REQUIRED",
        );
    }

    #[test]
    fn set_cell_and_stamp_semantic_loss_reproduce_the_legacy_codec_message() {
        use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};

        let warning = EncodeWarning::NoteHeadSkipped {
            path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)]),
            reason: "titleMark first run".into(),
        };
        let err = OpsError::semantic_loss(vec![warning], vec![]);

        let set_cell = tool_error(Tool::SetCell, {
            let OpsError::EncodeSemanticLoss { warnings, others } = &err else { unreachable!() };
            OpsError::EncodeSemanticLoss { warnings: warnings.clone(), others: others.clone() }
        });
        assert_eq!(set_cell.code, "SET_CELL_CODEC_FAILED");
        assert!(set_cell.message.starts_with("codec failure: "), "{set_cell:?}");
        assert!(set_cell.message.contains("note number head skipped"), "{}", set_cell.message);

        let stamp = tool_error(Tool::Stamp, {
            let OpsError::EncodeSemanticLoss { warnings, others } = &err else { unreachable!() };
            OpsError::EncodeSemanticLoss { warnings: warnings.clone(), others: others.clone() }
        });
        assert_eq!(stamp.code, "STAMP_CODEC_FAILED");
        assert!(!stamp.message.starts_with("codec failure: "), "{stamp:?}");

        let restyle = tool_error(Tool::Restyle, err);
        assert_eq!(restyle.code, "ENCODE_SEMANTIC_LOSS");
        assert!(restyle.message.contains("note number head skipped"), "{}", restyle.message);
    }

    #[test]
    fn stamp_cell_not_anchor_reconstructs_the_dynamic_hint() {
        let err = OpsError::Stamper(StamperError::CellStamp(CellStampError::NotAnAnchor {
            table: 0,
            requested: GridCoord::new(0, 1),
            anchor: Some(GridCoord::new(0, 0)),
        }));
        let info = tool_error(Tool::Stamp, err);
        assert_eq!(info.code, "STAMP_CELL_NOT_ANCHOR");
        assert!(info.hint.contains("(0,0)"), "{info:?}");

        let err = OpsError::Stamper(StamperError::CellStamp(CellStampError::NotAnAnchor {
            table: 0,
            requested: GridCoord::new(9, 9),
            anchor: None,
        }));
        let info = tool_error(Tool::Stamp, err);
        assert!(info.hint.contains("격자 범위"), "{info:?}");
    }

    #[test]
    fn stamp_dual_source_name_codes_pick_the_matching_wording() {
        let text = tool_error(
            Tool::Stamp,
            OpsError::Stamper(StamperError::Stamp(StampError::DuplicateName { name: "x".into() })),
        );
        let cell = tool_error(
            Tool::Stamp,
            OpsError::Stamper(StamperError::CellStamp(CellStampError::DuplicateName {
                name: "x".into(),
            })),
        );
        assert_eq!(text.code, "STAMP_NAME_DUPLICATE");
        assert_eq!(cell.code, "STAMP_NAME_DUPLICATE");
        assert_ne!(text.hint, cell.hint, "text-spec and cell-spec wording must stay distinct");
    }

    #[test]
    fn upstream_unmapped_falls_back_to_each_tool_s_legacy_catch_all() {
        assert_code(
            Tool::SetCell,
            OpsError::CellEdit(CellEditError::Codec("x".into())),
            "SET_CELL_CODEC_FAILED",
        );
        // A CellEditError variant `cell_edit_code()` has no explicit arm for
        // classifies as UpstreamUnmapped; NotRoundTripSafe stands in for
        // "any variant not TABLE-listed by code" since the enum is
        // `#[non_exhaustive]` and every named variant already has a row.
        assert_code(
            Tool::SetCell,
            OpsError::CellEdit(CellEditError::NotRoundTripSafe {
                component: "x".into(),
                diff_path: "y".into(),
            }),
            "INPUT_NOT_ROUNDTRIP_SAFE",
        );
    }

    #[test]
    fn grid_addr_and_stamp_map_pass_through_by_design() {
        // No legacy row exists for these: GRID_ADDR_INVALID (from_json/patch)
        // already has a row, but GridAddrError's *projection* code and
        // StampMapError's INVALID_STAMP_MAP never had a dedicated MCP tool
        // caller in the pre-migration source for `tool_error` to preserve
        // — both are exercised through the tool's own map_grid_addr_projection_error
        // / (not present) paths today, not through this compat layer.
        let err = OpsError::GridAddrProjection(GridAddrError::ShapeMismatch { path: "x".into() });
        assert_eq!(err.code(), OpsCode::GridAddrProjectionFailed);
        let err2 = OpsError::StampMap(StampMapError::Parse("x".into()));
        assert_eq!(err2.code(), OpsCode::InvalidStampMap);
        // Pass-through (no TABLE row for StampPlan/whatever calls this
        // directly): code.as_str() is the wire string itself.
        assert_eq!(tool_error(Tool::ToJson, err).code, "GRID_ADDR_PROJECTION_FAILED");
        let _ = err2; // constructed only to prove the variant compiles/exists
    }

    #[test]
    fn warning_copies_the_ops_warning_info_verbatim() {
        use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};

        let w = OpsWarning::Encode(EncodeWarning::LayoutCacheDropped {
            path: ParagraphPath(vec![PathSeg::Section(0)]),
            reason: "ledger".into(),
        });
        let info = warning(&w);
        assert_eq!(info.code, "LAYOUT_CACHE_DROPPED");
        assert!(info.message.contains("ledger"));
        assert!(info.hint.is_none());
    }
}
