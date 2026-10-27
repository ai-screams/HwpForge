//! Canonical operation codes with frontend-specific messages and hints.

use hwpforge::ops::{OpsError, OpsWarning};
use hwpforge_foundation::diagnostics::OpsCode;
use hwpforge_smithy_hwpx::stamp::{CellStampError, StampError, StamperError};
use hwpforge_smithy_hwpx::{FillError, SectionWorkflowError};

use crate::output::{ToolErrorInfo, ToolWarningInfo};

pub(crate) const READ_TARGET_REQUIRED_MESSAGE: &str = "Pass exactly one of section, table, field";

pub(crate) const READ_PARAS_WITHOUT_SECTION_MESSAGE: &str = "paras requires section";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// Tool context for response hints.
pub enum Tool {
    ConvertMd,
    Inspect,
    Outline,
    Read,
    Fields,
    ToJson,
    FromJson,
    Patch,
    Diff,
    Fill,
    SetCell,
    InsertPara,
    DeletePara,
    StampPlan,
    Stamp,
    Templates,
    Restyle,
    Validate,
    ToMd,
}

struct Row {
    tool: Tool,
    code: OpsCode,
    hint: Option<&'static str>,
}

macro_rules! row {
    ($tool:ident, $code:ident) => {
        Row { tool: Tool::$tool, code: OpsCode::$code, hint: None }
    };
    ($tool:ident, $code:ident, $hint:literal) => {
        Row { tool: Tool::$tool, code: OpsCode::$code, hint: Some($hint) }
    };
}

#[rustfmt::skip]
const TABLE: &[Row] = &[
    row!(ConvertMd, PresetNotFound, "Use hwpforge_templates to see available presets."),
    row!(ConvertMd, MdDecodeFailed, "Check Markdown syntax. Use GFM (GitHub Flavored Markdown)."),
    row!(ConvertMd, StyleStoreFailed, "Check paragraph list references in the resolved style registry."),
    row!(ConvertMd, StyleRebindFailed, "Document style indices do not match the generated HWPX style store."),
    row!(ConvertMd, ValidationFailed, "Check document structure."),
    row!(ConvertMd, EncodeFailed, "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),

    row!(Inspect, DecodeFailed, "Check that the file is a valid HWPX document."),

    row!(Outline, DecodeFailed, "Check that the file is valid HWPX. This server reads HWPX only — convert an HWP5 (.hwp) file first with the CLI: hwpforge convert-hwp5 <in.hwp> -o <out.hwpx>."),

    row!(Read, DecodeFailed, "Check that the file is valid HWPX. This server reads HWPX only — convert an HWP5 (.hwp) file first with the CLI: hwpforge convert-hwp5 <in.hwp> -o <out.hwpx>."),
    row!(Read, ReadTargetRequired, "section reads a paragraph range; table reads a grid text matrix; field reads a named click-here field."),
    row!(Read, ReadParasWithoutSection, "Pass section together with paras."),
    row!(Read, ReadParasInvalid, "Use \"A..B\" (inclusive) or a single \"N\"."),
    row!(Read, SectionOutOfRange, "Use hwpforge_outline to see section indexes."),
    row!(Read, ReadParaRangeInvalid, "Use hwpforge_outline to see paragraph counts per section."),
    row!(Read, TableNotFound, "Use hwpforge_outline to see table ordinals."),
    row!(Read, TableGridInvalid, "This table's strict grid cannot be derived; use hwpforge_to_json."),
    row!(Read, FieldNotFound, "Use hwpforge_fields to list available field names."),

    row!(Fields, DecodeFailed, "Check that the file is valid HWPX. This server reads HWPX only — convert an HWP5 (.hwp) file first with the CLI: hwpforge convert-hwp5 <in.hwp> -o <out.hwpx>."),

    row!(ToJson, DecodeFailed, "Check that the file is a valid HWPX document."),

    row!(ToJson, JsonSerializeFailed, "This may be a bug."),
    row!(ToJson, GridAddrProjectionFailed, "This may be a bug."),

    row!(FromJson, JsonParseFailed, "Ensure JSON matches the ExportedDocument schema from hwpforge_to_json output."),
    row!(FromJson, GridAddrInvalid, "Grid addresses come from hwpforge_to_json output; after structural edits, drop the stale addr fields (or re-export) and retry."),
    row!(FromJson, ValidationFailed, "Check document structure."),
    row!(FromJson, EncodeFailed, "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),

    row!(Patch, JsonParseFailed, "Ensure the JSON matches the ExportedSection schema from hwpforge_to_json output."),
    row!(Patch, GridAddrInvalid, "Grid addresses come from hwpforge_to_json output; after structural edits, drop the stale addr fields (or re-export) and retry."),
    row!(Patch, DecodeFailed, "Check that the base file is valid HWPX."),
    row!(Patch, PatchFailed, "Re-export the target section with the current hwpforge_to_json tool so preservation metadata is embedded. Structural/style changes still require a broader rebuild workflow."),

    row!(Diff, DecodeFailed, "Both inputs must be valid HWPX. This server reads HWPX only — convert an HWP5 (.hwp) file first with the CLI: hwpforge convert-hwp5 <in.hwp> -o <out.hwpx>."),

    row!(Fill, NoValues, "Pass at least one name\u{2192}value pair. Use hwpforge_fields to discover names."),
    row!(Fill, EmptyFieldValue, "빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요."),
    row!(Fill, FieldNameAmbiguous, "같은 이름의 누름틀이 여러 개라 대상이 모호합니다 — 문서에서 이름을 유일하게 하세요."),
    row!(Fill, FieldNotFillable, "병합-run 모호 필드 또는 빈 본문(patch 슬롯 없음)이라 이 필드는 채우기 표면이 아닙니다 — 한컴에서 열어 다시 저장한 뒤 재시도하세요. 서버 안에서 끝내려면 hwpforge_to_json(section 없이 전체 문서) → 편집 → hwpforge_from_json 재구성이 가능하지만 이미지 바이너리는 유지되지 않습니다 (이미지를 지키는 재구성은 CLI hwpforge from-json --base)."),
    row!(Fill, DecodeFailed, "Check that the file is valid HWPX."),
    row!(Fill, FillFailed, "Check that the file is valid HWPX."),

    row!(SetCell, InvalidSetCellMap, "Pass at least one CellSpec: {table, at|right_of|below, text}."),
    row!(SetCell, TableNotFound, "표 서수는 hwpforge_to_json export 의 문서 순서 0-base 입니다."),
    row!(SetCell, TableGridInvalid, "이 표는 셀 span 이 well-formed 격자를 이루지 않아 주소 지정이 불가합니다."),
    row!(SetCell, CellNotFound, "좌표는 병합 전 논리 격자 0-base — export 의 addr 값을 쓰세요."),
    row!(SetCell, CellLabelAmbiguous, "라벨이 여러 셀과 일치합니다 — at 좌표로 직접 지정하세요."),
    row!(SetCell, CellHasNonTextContent, "표/이미지/컨트롤이 든 셀은 파괴 방지를 위해 교체를 거부합니다."),
    row!(SetCell, CellTargetDuplicate, "두 편집이 같은 앵커 셀로 resolve 됐습니다."),
    row!(SetCell, CellTargetConflict, "바깥 셀 교체가 다른 편집이 노리는 중첩 표를 파괴합니다."),
    row!(SetCell, DecodeFailed, "Check that the file is valid HWPX."),
    row!(Stamp, DecodeFailed, "Check that the file is valid HWPX."),
    row!(StampPlan, DecodeFailed, "Check that the file is valid HWPX."),
    row!(InsertPara, DecodeFailed, "Check that the file is valid HWPX."),
    row!(DeletePara, DecodeFailed, "Check that the file is valid HWPX."),
    row!(SetCell, InputNotRoundtripSafe, "이 입력은 무손실 재인코드가 증명되지 않아 편집을 거부합니다 (fail-closed). 텍스트만 바꾸려면 hwpforge_to_json(section) → hwpforge_patch, 누름틀은 hwpforge_fill 을 쓰세요 — 둘 다 원본 ZIP 엔트리를 보존합니다."),
    row!(SetCell, InputEntriesNotCarried, "인코더가 carry 하지 않는 ZIP entry 가 있어 편집을 거부합니다 (fail-closed) — 한컴이 저장한 문서(Preview/*·META-INF/container.rdf)는 현재 이 도구의 대상이 아닙니다. 텍스트만 바꾸려면 hwpforge_to_json(section) → hwpforge_patch, 누름틀은 hwpforge_fill 을 쓰세요."),
    row!(SetCell, UpstreamUnmapped, "Report this as a bug."),

    row!(InsertPara, InsertTextRequired, "Use `text` for a single paragraph or `texts` for a contiguous block."),
    row!(DeletePara, DeleteNoTarget, "indices must be a non-empty list of top-level paragraph indices."),
    row!(InsertPara, InputNotRoundtripSafe, "Structural edits require a round-trip-safe input; this document has a codec fidelity gap. To change text instead, export the section with hwpforge_to_json and apply it with hwpforge_patch; to fill a click-here field, use hwpforge_fill — both preserve the original ZIP entries."),
    row!(DeletePara, InputNotRoundtripSafe, "Structural edits require a round-trip-safe input; this document has a codec fidelity gap. To change text instead, export the section with hwpforge_to_json and apply it with hwpforge_patch; to fill a click-here field, use hwpforge_fill — both preserve the original ZIP entries."),
    row!(DeletePara, ReferenceStranded, "This paragraph carries a bookmark/cross-ref/footnote; deleting it could strand a reference."),
    row!(DeletePara, HardBreakLoss, "This paragraph carries a hard page/column break."),
    row!(InsertPara, SectionPropertiesParagraph, "The section's first paragraph holds page setup; it cannot be deleted or displaced."),
    row!(DeletePara, SectionPropertiesParagraph, "The section's first paragraph holds page setup; it cannot be deleted or displaced."),
    row!(DeletePara, EmptySection, "A section must keep at least one paragraph."),
    row!(InsertPara, MultiParagraphText, "Insert one paragraph per call; text may not contain line breaks."),
    row!(InsertPara, ParagraphOutOfRange, "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, ParagraphOutOfRange, "Use hwpforge_outline to see section and paragraph counts."),
    row!(InsertPara, SectionOutOfRange, "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, SectionOutOfRange, "Use hwpforge_outline to see section and paragraph counts."),
    row!(DeletePara, DuplicateTarget, "Each paragraph index may appear once per batch."),
    row!(InsertPara, SelfVerifyFailed, "The edit did not verify; no output was written."),
    row!(DeletePara, SelfVerifyFailed, "The edit did not verify; no output was written."),

    row!(InsertPara, InsertBeforeSectionProperties, "The section's first paragraph holds page setup; it cannot be deleted or displaced."),

    row!(InsertPara, SpanCountMismatch, "The structural edit was refused."),
    row!(DeletePara, SpanCountMismatch, "The structural edit was refused."),

    row!(InsertPara, InputEntriesNotCarried, "The structural edit was refused: the encoder does not carry every ZIP entry of this document, so a document saved by Hancom (Preview/*, META-INF/container.rdf) is out of scope for this tool today. To change text instead, export the section with hwpforge_to_json and apply it with hwpforge_patch; to fill a click-here field, use hwpforge_fill — both preserve the original ZIP entries."),
    row!(DeletePara, InputEntriesNotCarried, "The structural edit was refused: the encoder does not carry every ZIP entry of this document, so a document saved by Hancom (Preview/*, META-INF/container.rdf) is out of scope for this tool today. To change text instead, export the section with hwpforge_to_json and apply it with hwpforge_patch; to fill a click-here field, use hwpforge_fill — both preserve the original ZIP entries."),
    row!(InsertPara, UpstreamUnmapped, "The structural edit was refused."),
    row!(DeletePara, UpstreamUnmapped, "The structural edit was refused."),


    row!(Stamp, InputNotRoundtripSafe, "이 입력은 무손실 재인코드가 증명되지 않아 스탬핑을 거부합니다 (fail-closed). 텍스트는 hwpforge_to_json(section) → hwpforge_patch, 누름틀은 hwpforge_fill을 사용하세요"),
    row!(Stamp, InputEntriesNotCarried, "재인코드 시 유실될 ZIP 엔트리가 있어 거부됩니다 (fail-closed) — 한컴이 저장한 문서(Preview/*·META-INF/container.rdf)는 현재 이 도구의 대상이 아닙니다. 텍스트만 바꾸려면 hwpforge_to_json(section) → hwpforge_patch, 누름틀은 hwpforge_fill 을 쓰세요."),
    row!(Stamp, StampManifestInvariant, "Report this as a bug — the output inventory violated an invariant."),
    row!(Stamp, StampSourceHashMismatch, "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행해 source_sha256 을 갱신하세요."),
    row!(Stamp, SelfVerifyFailed, "산출물 검증 실패 — 코덱 버그 가능성이 있어 무출력으로 거부했습니다."),
    row!(Stamp, TableNotFound, "hwpforge_stamp_plan 의 cells[].table 서수를 사용하세요."),
    row!(Stamp, TableGridInvalid, "이 표는 논리 격자를 만들 수 없어 셀 스탬핑 대상이 아닙니다."),
    row!(Stamp, StampCellNotEmpty, "클래스-B 대상은 whitespace-only 빈 셀이어야 합니다."),
    row!(Stamp, StampLabelDrift, "문서가 변경됐습니다 — hwpforge_stamp_plan 을 다시 실행하세요."),
    row!(Stamp, StampCellNotCandidate, "ignore 는 live 후보에만 가능합니다."),
    row!(Stamp, CellTargetDuplicate, "같은 셀을 두 번 분류했습니다."),
    row!(Stamp, StampNameEmpty, "빈 이름은 허용되지 않습니다."),
    row!(Stamp, StampHintBlank, "빈 셀엔 마커가 없어 hint 가 필수입니다 — plan 의 suggested_hint 를 참고하세요."),
    row!(Stamp, StampSpecStale, "문서가 변경됐거나 span 이 어긋났습니다 — hwpforge_stamp_plan 을 다시 실행하세요."),
    row!(Stamp, StampMarkerMismatch, "spec 의 marker 는 문서의 현재 텍스트와 일치해야 합니다."),
    row!(Stamp, StampSpecDuplicate, "같은 후보를 두 번 분류했습니다."),
    row!(Stamp, UpstreamUnmapped, "Unexpected failure."),

    row!(Templates, PresetNotFound, "Available presets: default, modern, classic, latest. Use hwpforge_templates without a name to list all."),

    row!(Restyle, PresetNotFound, "Use hwpforge_templates to see available presets."),
    row!(Restyle, DecodeFailed, "Check that the file is a valid HWPX document."),
    row!(Restyle, NoFonts, "The HWPX file may be malformed. Use hwpforge_validate to check."),
    row!(Restyle, ValidationFailed, "Check document structure."),
    row!(Restyle, EncodeFailed, "This may be a bug. Please report at https://github.com/ai-screams/HwpForge/issues"),

    row!(ToMd, DecodeFailed, "Check that the file is a valid HWPX document."),
    row!(ToMd, ValidationFailed, "The HWPX document structure is invalid."),
];

#[must_use]
pub fn tool_error(tool: Tool, err: OpsError) -> ToolErrorInfo {
    let canonical = err.code();
    if let OpsError::EncodeSemanticLoss { warnings, .. } = &err {
        if let Some(info) = semantic_loss_error(tool, warnings) {
            return info;
        }
    }

    if tool == Tool::Validate {
        if let OpsError::Decode(_) = &err {
            return ToolErrorInfo::new(
                err.code(),
                err.to_string(),
                "Check that the file is valid HWPX. This server reads HWPX only — convert an HWP5 (.hwp) file first with the CLI: hwpforge convert-hwp5 <in.hwp> -o <out.hwpx>.",
            );
        }
    }

    match &err {
        OpsError::Fill(FillError::UnknownField { available, .. }) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                format!(
                    "Available fields: [{}]. Use hwpforge_fields to list them.",
                    available.join(", ")
                ),
            );
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange { sections, .. }) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                format!("Valid range: 0..={}", sections.saturating_sub(1)),
            );
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
            requested,
            actual,
        }) => {
            return ToolErrorInfo::new(canonical,
                err.to_string(),
                format!(
                    "Use section: {actual} to match the JSON, or re-export section {requested} with hwpforge_to_json."
                ),
            );
        }

        OpsError::SectionWorkflow(SectionWorkflowError::Decode { .. })
        | OpsError::SectionWorkflow(SectionWorkflowError::PreservingPatch(_)) => {}

        OpsError::SectionWorkflow(_) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "Update hwpforge so this MCP binding understands the newer section workflow error.",
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
            return ToolErrorInfo::new(canonical, err.to_string(), hint);
        }

        OpsError::Stamper(StamperError::Stamp(StampError::DuplicateName { .. })) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "필드 이름은 spec 전체에서 유일해야 합니다.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::DuplicateName { .. })) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "필드 이름은 text+cells 전체에서 유일해야 합니다.",
            );
        }
        OpsError::Stamper(StamperError::Stamp(StampError::NameCollision { .. })) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 기존 이름을 확인하세요.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NameCollision { .. })) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "기존 누름틀과 이름이 겹칩니다 — hwpforge_fields 로 확인하세요.",
            );
        }
        OpsError::Stamper(StamperError::Stamp(StampError::UncoveredCandidate { .. })) => {
            return ToolErrorInfo::new(canonical,
                err.to_string(),
                "모든 무가드 후보는 이름 또는 ignore 로 분류해야 합니다 — hwpforge_stamp_plan 출력을 빠짐없이 사용하세요.",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::UncoveredCandidate {
            ..
        })) => {
            return ToolErrorInfo::new(
                canonical,
                err.to_string(),
                "모든 무가드 셀 후보는 이름 또는 ignore 로 분류해야 합니다.",
            );
        }

        OpsError::Rejected { code: OpsCode::ReadTargetRequired, .. } => {
            return ToolErrorInfo::new(canonical,
                READ_TARGET_REQUIRED_MESSAGE,
                "section reads a paragraph range; table reads a grid text matrix; field reads a named click-here field.",
            );
        }
        OpsError::Rejected { code: OpsCode::ReadParasWithoutSection, .. } => {
            return ToolErrorInfo::new(
                canonical,
                READ_PARAS_WITHOUT_SECTION_MESSAGE,
                "Pass section together with paras.",
            );
        }
        _ => {}
    }

    let code = err.code();
    let message = err.to_string();
    match TABLE.iter().find(|row| row.tool == tool && row.code == code) {
        Some(row) => {
            ToolErrorInfo::new(code, message, row.hint.or_else(|| err.hint()).unwrap_or_default())
        }
        None => ToolErrorInfo::new(code, message, err.hint().unwrap_or_default()),
    }
}

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
            Some(ToolErrorInfo::new(OpsCode::EncodeSemanticLoss, message, "Report this as a bug."))
        }
        Tool::Stamp => {
            let message = match warnings.first() {
                Some(w) => {
                    format!("encode produced a semantic-loss warning (fail-closed): {}", w.message)
                }
                None => "encode produced a semantic-loss warning (fail-closed)".to_string(),
            };
            Some(ToolErrorInfo::new(
                OpsCode::EncodeSemanticLoss,
                message,
                "Check that the file is valid HWPX.",
            ))
        }
        Tool::Restyle => {
            let message =
                warnings.iter().map(|w| w.message.as_str()).collect::<Vec<_>>().join("; ");
            Some(ToolErrorInfo::new(OpsCode::EncodeSemanticLoss,
                message,
                "The restyled document would lose footnote/endnote numbering or TOC marks; fix the \
                 source document or restyle a document without those constructs.",
            ))
        }
        _ => None,
    }
}

#[must_use]
pub fn warning(w: &OpsWarning) -> ToolWarningInfo {
    w.info().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_smithy_hwpx::grid_addr::GridAddrError;
    use hwpforge_smithy_hwpx::stamp::StampMapError;
    use hwpforge_smithy_hwpx::{
        CellEditError, HwpxError, PatchError, ReadError, StructuralEditError,
    };

    fn assert_code(tool: Tool, err: OpsError, expected: &str) {
        let got = tool_error(tool, err);
        assert_eq!(got.code, expected, "{tool:?}: {got:?}");
    }

    #[test]
    // 이것을 실패시키는 것: 같은 (도구, 코드) 행 중복
    fn table_has_no_duplicate_tool_code_rows() {
        let mut seen = std::collections::HashSet::new();
        for row in TABLE {
            let key = (row.tool, row.code.as_str());
            assert!(seen.insert(key), "duplicate row: {key:?}");
        }
    }

    #[test]
    fn w3_editors_share_decode_and_admission_fixtures() {
        // 이것을 실패시키는 것: E3 structural admission 호출 목적을 Output으로 변경.
        use crate::tools::{set_cell, stamp, structural};
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../hwpforge/tests/data");
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("out.hwpx");
        let output = out.to_str().unwrap();
        let spec = hwpforge_smithy_hwpx::CellSpec {
            table: 0,
            target: hwpforge_smithy_hwpx::CellTarget::At(
                hwpforge_core::table::grid::GridCoord::new(0, 0),
            ),
            text: "text".into(),
        };
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
                .unwrap();
        let check = |id: &str, error: ToolErrorInfo| {
            let case =
                manifest["cases"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap();
            assert_eq!(error.code, case["expected_code"].as_str().unwrap(), "{id}");
            assert!(!error.hint.contains("Report this as a bug."));
            assert!(!out.exists());
        };
        for (fixture, ids) in [
            (
                "corrupt-input.hwpx",
                [
                    "cell.input_decode",
                    "structural.input_decode",
                    "structural_delete.input_decode",
                    "stamp.input_decode",
                ],
            ),
            (
                "edit-empty-section.hwpx",
                [
                    "cell.admission",
                    "structural.admission",
                    "structural_delete.admission",
                    "stamp.admission",
                ],
            ),
        ] {
            let input = data.join(fixture);
            let file = input.to_str().unwrap();
            check(
                ids[0],
                set_cell::run_set_cell(file, std::slice::from_ref(&spec), output).err().unwrap(),
            );
            check(
                ids[1],
                structural::run_insert_para(file, 0, 0, false, Some("text"), None, output)
                    .err()
                    .unwrap(),
            );
            check(ids[2], structural::run_delete_para(file, 0, &[0], output).err().unwrap());
            check(ids[3], stamp::run_stamp(file, &[], &[], None, output, None).err().unwrap());
        }
        // 이것을 실패시키는 것: F4 v2 admission 호출 목적을 Output으로 변경.
        // 이것을 실패시키는 것: F6 MCP stamp admission 힌트를 코덱 갭 대기로 복원.
        let request: hwpforge_smithy_hwpx::stamp::StampRequestV2 =
            serde_json::from_str(include_str!("../../hwpforge/tests/data/stamp-v2-admission.json"))
                .unwrap();
        let error = stamp::run_stamp(
            data.join("edit-empty-section.hwpx").to_str().unwrap(),
            &request.text,
            &request.cells,
            Some(&request.source_sha256),
            output,
            None,
        )
        .err()
        .unwrap();
        assert_eq!(error.hint, "이 입력은 무손실 재인코드가 증명되지 않아 스탬핑을 거부합니다 (fail-closed). 텍스트는 hwpforge_to_json(section) → hwpforge_patch, 누름틀은 hwpforge_fill을 사용하세요");
        check("stamp.v2_admission", error);
        check(
            "stamp_plan.input_decode",
            stamp::run_stamp_plan(data.join("corrupt-input.hwpx").to_str().unwrap()).err().unwrap(),
        );
        for name in ["depth-table-42", "depth-table-33", "depth-group-32", "depth-xml-broken"] {
            check(
                &format!("cell.{name}"),
                set_cell::run_set_cell(
                    data.join(format!("{name}.hwpx")).to_str().unwrap(),
                    std::slice::from_ref(&spec),
                    output,
                )
                .err()
                .unwrap(),
            );
        }
    }

    #[test]
    fn validate_decode_failure_matches_the_new_codes_snapshot() {
        let err = OpsError::decode(HwpxError::Zip("not a zip file".into()));
        let got = tool_error(Tool::Validate, err);
        assert_eq!(got.code, "DECODE_FAILED");

        let shared = TABLE
            .iter()
            .find(|r| r.tool == Tool::Outline && r.code == OpsCode::DecodeFailed)
            .and_then(|r| r.hint)
            .expect("outline's DecodeFailed row carries the shared decode hint");
        assert_eq!(got.hint, shared);
        assert!(got.hint.contains("hwpforge convert-hwp5"), "{got:?}");
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
    fn read_shape_rejections_keep_the_mcp_wording_whoever_produced_them() {
        let from_ops = OpsError::Rejected {
            code: OpsCode::ReadTargetRequired,
            reason: "a producer's own wording for the target rule".into(),
        };
        assert_eq!(tool_error(Tool::Read, from_ops).message, READ_TARGET_REQUIRED_MESSAGE);

        let from_ops = OpsError::Rejected {
            code: OpsCode::ReadParasWithoutSection,
            reason: "a producer's own wording for the paras rule".into(),
        };
        assert_eq!(tool_error(Tool::Read, from_ops).message, READ_PARAS_WITHOUT_SECTION_MESSAGE);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
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
            "SECTION_OUT_OF_RANGE",
        );
        assert_code(
            Tool::Read,
            OpsError::Read(ReadError::FieldNotFound { name: "x".into(), available: vec![] }),
            "FIELD_NOT_FOUND",
        );
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
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
                    PatchError::Rejected(HwpxError::InvalidStructure {
                        detail: "preservation metadata version".into(),
                    }),
                )),
                "PATCH_FAILED",
            );

            assert_code(
                tool,
                OpsError::SectionWorkflow(SectionWorkflowError::Decode { detail: "x".into() }),
                "DECODE_FAILED",
            );
        }

        for err in [
            OpsError::SectionWorkflow(SectionWorkflowError::Decode { detail: "x".into() }),
            OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange {
                requested: 1,
                sections: 1,
            }),
            OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
                requested: 0,
                actual: 1,
            }),
            OpsError::SectionWorkflow(SectionWorkflowError::PreservingPatch(PatchError::Rejected(
                HwpxError::InvalidStructure { detail: "x".into() },
            ))),
        ] {
            assert_ne!(err.code(), OpsCode::UpstreamUnmapped, "{err}");
        }
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn structural_errors_preserve_distinct_range_codes() {
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
            let expected = if tool == Tool::InsertPara {
                "SECTION_OUT_OF_RANGE"
            } else {
                "PARAGRAPH_OUT_OF_RANGE"
            };
            assert_code(tool, err, expected);
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
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn insert_before_section_properties_matches_the_legacy_combined_arm() {
        let info = tool_error(
            Tool::InsertPara,
            OpsError::StructuralEdit(StructuralEditError::InsertBeforeSectionProperties {
                section: 0,
                index: 0,
            }),
        );
        assert_eq!(info.code, "INSERT_BEFORE_SECTION_PROPERTIES");
        assert_eq!(
            info.hint,
            "The section's first paragraph holds page setup; it cannot be deleted or displaced."
        );
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn structural_uncarried_entries_use_canonical_code() {
        for tool in [Tool::InsertPara, Tool::DeletePara] {
            let info = tool_error(
                tool,
                OpsError::StructuralEdit(StructuralEditError::UncarriedZipEntries {
                    entries: vec!["Contents/section0.xml".into()],
                }),
            );
            assert_eq!(info.code, "INPUT_ENTRIES_NOT_CARRIED", "{tool:?}");
            assert!(info.hint.contains("hwpforge_patch"), "{tool:?}: {info:?}");
            assert!(info.hint.contains("hwpforge_fill"), "{tool:?}: {info:?}");

            let generic = tool_error(
                tool,
                OpsError::StructuralEdit(StructuralEditError::SpanCountMismatch {
                    section: 0,
                    decoded: 2,
                    wire: 1,
                }),
            );
            assert_eq!(generic.code, "SPAN_COUNT_MISMATCH", "{tool:?}");
            assert_eq!(generic.hint, "The structural edit was refused.", "{tool:?}");
        }
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn set_cell_and_stamp_semantic_loss_reproduce_the_legacy_codec_message() {
        use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};

        let note_head = EncodeWarning::NoteHeadSkipped {
            path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)]),
            reason: "titleMark first run".into(),
        };
        let title_mark = EncodeWarning::TitleMarkSkipped {
            path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(1)]),
            reason: "second reason".into(),
        };
        let cache_dropped = EncodeWarning::LayoutCacheDropped {
            path: ParagraphPath(vec![PathSeg::Section(0)]),
            reason: "ledger".into(),
        };

        let clone_err = |e: &OpsError| {
            let OpsError::EncodeSemanticLoss { warnings, others } = e else { unreachable!() };
            OpsError::EncodeSemanticLoss { warnings: warnings.clone(), others: others.clone() }
        };

        struct Case {
            name: &'static str,
            warnings: Vec<EncodeWarning>,
            others: Vec<EncodeWarning>,
            set_cell: &'static str,
            stamp: &'static str,
            restyle: &'static str,
        }
        let cases = [
            Case {
                name: "empty warnings",
                warnings: vec![],
                others: vec![],
                set_cell: "codec failure: encode produced a semantic-loss warning (fail-closed)",
                stamp: "encode produced a semantic-loss warning (fail-closed)",
                restyle: "",
            },
            Case {
                name: "single warning",
                warnings: vec![note_head.clone()],
                others: vec![],
                set_cell: "codec failure: encode produced a semantic-loss warning (fail-closed): \
                            note number head skipped at section[0].para[1]: titleMark first run",
                stamp: "encode produced a semantic-loss warning (fail-closed): note number head \
                        skipped at section[0].para[1]: titleMark first run",
                restyle: "note number head skipped at section[0].para[1]: titleMark first run",
            },
            Case {
                name: "multiple warnings — set_cell/stamp use only the first",
                warnings: vec![note_head.clone(), title_mark.clone()],
                others: vec![],
                set_cell: "codec failure: encode produced a semantic-loss warning (fail-closed): \
                            note number head skipped at section[0].para[1]: titleMark first run",
                stamp: "encode produced a semantic-loss warning (fail-closed): note number head \
                        skipped at section[0].para[1]: titleMark first run",
                restyle: "note number head skipped at section[0].para[1]: titleMark first run; \
                          titleMark skipped at section[0].para[1]: second reason",
            },
            Case {
                name: "non-empty others is ignored",
                warnings: vec![note_head.clone()],
                others: vec![cache_dropped.clone()],
                set_cell: "codec failure: encode produced a semantic-loss warning (fail-closed): \
                            note number head skipped at section[0].para[1]: titleMark first run",
                stamp: "encode produced a semantic-loss warning (fail-closed): note number head \
                        skipped at section[0].para[1]: titleMark first run",
                restyle: "note number head skipped at section[0].para[1]: titleMark first run",
            },
        ];

        for case in cases {
            let err = OpsError::semantic_loss(case.warnings, case.others);

            let set_cell = tool_error(Tool::SetCell, clone_err(&err));
            assert_eq!(set_cell.code, "ENCODE_SEMANTIC_LOSS", "{}", case.name);
            assert_eq!(set_cell.message, case.set_cell, "{}", case.name);
            assert_eq!(set_cell.hint, "Report this as a bug.", "{}", case.name);

            let stamp = tool_error(Tool::Stamp, clone_err(&err));
            assert_eq!(stamp.code, "ENCODE_SEMANTIC_LOSS", "{}", case.name);
            assert_eq!(stamp.message, case.stamp, "{}", case.name);
            assert_eq!(stamp.hint, "Check that the file is valid HWPX.", "{}", case.name);

            let restyle = tool_error(Tool::Restyle, err);
            assert_eq!(restyle.code, "ENCODE_SEMANTIC_LOSS", "{}", case.name);
            assert_eq!(restyle.message, case.restyle, "{}", case.name);
        }
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
    fn set_cell_codec_and_admission_keep_distinct_codes() {
        // 이것을 실패시키는 것: T8 CellEditError::Codec 매핑을 InvalidInput으로 변경.
        assert_code(
            Tool::SetCell,
            OpsError::CellEdit(CellEditError::Codec(hwpforge_smithy_hwpx::CodecFailure {
                stage: hwpforge_smithy_hwpx::CodecStage::Encode,
                code: hwpforge_smithy_hwpx::HwpxErrorCode::Zip,
                detail: "x".into(),
            })),
            "ENCODE_FAILED",
        );
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
        let err = OpsError::GridAddrProjection(GridAddrError::ShapeMismatch { path: "x".into() });
        assert_eq!(err.code(), OpsCode::GridAddrProjectionFailed);
        let err2 = OpsError::StampMap(StampMapError::Parse("x".into()));
        assert_eq!(err2.code(), OpsCode::InvalidStampMap);

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

    #[test]
    fn all_policy_rows_match_baseline() {
        // 이것을 실패시키는 것: TABLE 행의 코드·힌트·종료 값 변경, 행 추가 또는 기준선에서 TABLE 행 삭제.
        let mut covered = Vec::new();
        for line in include_str!("../tests/data/codes-0.18.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            let fields: Vec<_> = line.split('\t').collect();
            // INVALID_PARAMS belongs to CheckedParams, exercised below through the router.
            if fields[2].starts_with("params.") {
                continue;
            }
            let operation = fields[0];
            let expected = fields[1];
            let command = match operation {
                "convert" => Tool::ConvertMd,
                "inspect" => Tool::Inspect,
                "outline" => Tool::Outline,
                "read" => Tool::Read,
                "fields" => Tool::Fields,
                "to-json" => Tool::ToJson,
                "from-json" => Tool::FromJson,
                "patch" => Tool::Patch,
                "diff" => Tool::Diff,
                "fill" => Tool::Fill,
                "set-cell" => Tool::SetCell,
                "insert-para" => Tool::InsertPara,
                "delete-para" => Tool::DeletePara,
                "stamp-plan" => Tool::StampPlan,
                "stamp" => Tool::Stamp,
                "templates" => Tool::Templates,
                "restyle" => Tool::Restyle,
                "validate" => Tool::Validate,
                "to-md" => Tool::ToMd,
                other => panic!("unconsumed mcp operation: {other}"),
            };
            let code = OpsCode::lookup(expected).unwrap();
            covered.push((command, code));
            let got = tool_error(
                command,
                OpsError::Rejected { code, reason: "injected adapter boundary".into() },
            );
            assert_eq!(got.code, expected, "{line}");
            let hint: Option<String> = serde_json::from_str(fields[3]).unwrap();
            match hint.as_deref() {
                Some("@dynamic" | "@ops") => {}
                expected => assert_eq!(
                    Some(got.hint.as_str()).filter(|hint| !hint.is_empty()),
                    expected,
                    "{line}"
                ),
            }
        }
        for row in TABLE {
            assert!(
                covered.contains(&(row.tool, row.code)),
                "uncovered policy row: {:?}/{:?}",
                row.tool,
                row.code
            );
        }
    }

    #[tokio::test]
    async fn manifest_surface_cases_are_consumed() {
        // 이것을 실패시키는 것: read.section 소비 누락·기준선 코드 변경, 미검토 unreachable ID 추가.
        // 이것을 실패시키는 것: unreachable 이유 비우기, manifest에 없는 기준선 ID 추가.
        // 이것을 실패시키는 것: params 기준선 힌트를 변경.
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
                .unwrap();
        let baseline: Vec<Vec<&str>> = include_str!("../tests/data/codes-0.18.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.split('\t').collect())
            .collect();
        let expected_ids: std::collections::BTreeSet<_> = manifest["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["surfaces"].get("mcp").is_some())
            .map(|case| case["id"].as_str().unwrap())
            .collect();
        let mut consumed = std::collections::BTreeSet::new();
        let mut linked = std::collections::BTreeSet::new();
        for case in manifest["cases"].as_array().unwrap() {
            let Some(surface) = case["surfaces"].get("mcp") else { continue };
            let id = case["id"].as_str().unwrap();
            if surface["mode"] == "unreachable" {
                assert_eq!(id, "cell.draft_depth", "unhandled unreachable case: {id}");
                assert!(!surface["reason"].as_str().unwrap().is_empty());
            } else {
                let row = baseline
                    .iter()
                    .find(|row| row[2] == id)
                    .unwrap_or_else(|| panic!("unconsumed case: {id}"));
                assert_eq!(row[0], surface["operation"].as_str().unwrap(), "{id}");
                assert_eq!(row[1], case["expected_code"].as_str().unwrap(), "{id}");
                if id.starts_with("params.") {
                    assert_eq!(surface["mode"], "fixture", "{id}");
                    let got = crate::server::param_tests::assert_invalid_case(case).await;
                    assert_eq!(got.code, case["expected_code"], "{id}");
                    let hint: Option<String> = serde_json::from_str(row[3]).unwrap();
                    assert_eq!(got.hint, hint.unwrap_or_default(), "{id}");
                }
                linked.insert(id);
            }
            consumed.insert(id);
        }
        assert_eq!(consumed, expected_ids, "unconsumed surface case IDs");
        let baseline_ids = baseline.iter().map(|row| row[2]).filter(|id| *id != "-").collect();
        assert_eq!(linked, baseline_ids, "baseline links must name consumed cases");
    }
}
