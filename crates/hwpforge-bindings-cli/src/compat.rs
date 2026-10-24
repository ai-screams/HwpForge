//! Canonical operation codes with frontend-specific messages and hints.

use hwpforge::ops::OpsError;
use hwpforge_convert::ops::ConvertOpsError;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwpx::stamp::{CellStampError, StampError, StamperError};
use hwpforge_smithy_hwpx::{CellEditError, FillError, SectionWorkflowError};

use crate::error::{CliCode, CliError, CliOnly, ErrorCause};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// CLI command context for exit policy.
pub enum Command {
    AuditHwp5,
    CensusHwp5,
    ConvertHwp5,
    ToPdf,
    Convert,
    Inspect,
    ToJson,
    FromJson,
    Outline,
    Diff,
    DeletePara,
    InsertPara,
    Read,
    Fields,
    Fill,
    SetCell,
    StampPlan,
    Stamp,
    Patch,
    Templates,

    #[allow(dead_code, reason = "schema stays CLI-local; variant keeps the inventory exhaustive")]
    Schema,
    ToMd,

    Validate,
}

#[derive(Debug, Clone, Copy)]
enum Hint {
    None,

    Literal(&'static str),

    FromOps,
}

struct Row {
    cmd: Command,
    code: CliCode,
    hint: Hint,
    exit: i32,
}

const UNCARRIED_ENTRIES_HINT: &str = "한컴이 저장한 문서(`Preview/*`·`META-INF/container.rdf`)는 이 명령의 대상이 아닙니다 — 텍스트는 `to-json --section N` → 편집 → `patch`, 누름틀은 `fill` 로 바꾸세요 (둘 다 원본 ZIP 엔트리를 보존합니다)";

const UNKNOWN_PRESET_HINT: &str = "Available presets: default, modern, classic, latest";

macro_rules! row {
    ($cmd:ident, $code:ident, $exit:literal) => {
        Row {
            cmd: Command::$cmd,
            code: CliCode::Ops(OpsCode::$code),
            hint: Hint::None,
            exit: $exit,
        }
    };
    ($cmd:ident, $code:ident, $exit:literal, ops) => {
        Row {
            cmd: Command::$cmd,
            code: CliCode::Ops(OpsCode::$code),
            hint: Hint::FromOps,
            exit: $exit,
        }
    };
    ($cmd:ident, $code:ident, $exit:literal, $hint:literal) => {
        Row {
            cmd: Command::$cmd,
            code: CliCode::Ops(OpsCode::$code),
            hint: Hint::Literal($hint),
            exit: $exit,
        }
    };
    ($cmd:ident, $code:ident, $exit:literal, $hint:expr) => {
        Row {
            cmd: Command::$cmd,
            code: CliCode::Ops(OpsCode::$code),
            hint: Hint::Literal($hint),
            exit: $exit,
        }
    };
}

#[must_use]
fn resolved_hint(hint: Hint, ops_hint: Option<&'static str>) -> Option<&'static str> {
    match hint {
        Hint::None => None,
        Hint::Literal(text) => Some(text),
        Hint::FromOps => ops_hint,
    }
}

#[must_use]
fn lookup(cmd: Command, matches: impl Fn(&Row) -> bool) -> Option<&'static Row> {
    TABLE.iter().find(|row| row.cmd == cmd && matches(row))
}

#[rustfmt::skip]
const TABLE: &[Row] = &[

    row!(ConvertHwp5, Hwp5DecodeFailed, 2, "Check that the source is a supported HWP5 document and the output path is writable"),
    row!(ConvertHwp5, Hwp5ConvertFailed, 2, "Check that the source is a supported HWP5 document and the output path is writable"),

    row!(ToPdf, Hwp5DecodeFailed, 2),
    row!(ToPdf, Hwp5ConvertFailed, 2),
    row!(ToPdf, DecodeFailed, 2),
    row!(ToPdf, ValidationFailed, 2),
    row!(ToPdf, UnrecognizedFormat, 2, "to-pdf detects the format by content — the extension is only a hint"),
    row!(ToPdf, PdfRenderFailed, 2, "cacheless documents need a Hancom re-save; font errors may need --font-dir/--discovery or --degraded"),
    row!(ToPdf, InvalidDiscovery, 2),

    row!(Convert, PresetNotFound, 1, UNKNOWN_PRESET_HINT),
    row!(Convert, MdDecodeFailed, 2),
    row!(Convert, StyleStoreFailed, 2),
    row!(Convert, StyleRebindFailed, 2),
    row!(Convert, ValidationFailed, 2),
    row!(Convert, EncodeFailed, 2),

    row!(Inspect, DecodeFailed, 2, ops),

    row!(ToJson, DecodeFailed, 2),
    row!(ToJson, JsonSerializeFailed, 2, "Check for NaN/Infinity values in chart data"),
    row!(ToJson, GridAddrProjectionFailed, 2),

    row!(FromJson, JsonParseFailed, 2),
    row!(FromJson, GridAddrInvalid, 2, "Grid addresses come from to-json output; after structural edits, drop the stale addr fields (or re-export) and retry"),
    row!(FromJson, ValidationFailed, 2),
    row!(FromJson, DecodeFailed, 2),
    row!(FromJson, EncodeFailed, 2),

    row!(Outline, DecodeFailed, 2),

    row!(Diff, DecodeFailed, 2),

    row!(DeletePara, DecodeFailed, 2, ops),
    row!(InsertPara, DecodeFailed, 2, ops),
    row!(DeletePara, DeleteNoTarget, 1),
    row!(DeletePara, DuplicateTarget, 1),
    row!(DeletePara, ReferenceStranded, 1),
    row!(DeletePara, HardBreakLoss, 1),
    row!(DeletePara, EmptySection, 1),

    row!(InsertPara, InsertTextRequired, 1),
    row!(InsertPara, MultiParagraphText, 1),
    row!(InsertPara, InsertBeforeSectionProperties, 1),
    row!(DeletePara, SectionOutOfRange, 1),
    row!(InsertPara, SectionOutOfRange, 1),
    row!(DeletePara, ParagraphOutOfRange, 1),
    row!(InsertPara, ParagraphOutOfRange, 1),
    row!(DeletePara, InputNotRoundtripSafe, 1),
    row!(InsertPara, InputNotRoundtripSafe, 1),
    row!(DeletePara, InputEntriesNotCarried, 1, UNCARRIED_ENTRIES_HINT),
    row!(InsertPara, InputEntriesNotCarried, 1, UNCARRIED_ENTRIES_HINT),
    row!(DeletePara, SectionPropertiesParagraph, 1),
    row!(InsertPara, SectionPropertiesParagraph, 1),
    row!(DeletePara, SpanCountMismatch, 1),
    row!(InsertPara, SpanCountMismatch, 1),
    row!(DeletePara, SelfVerifyFailed, 1),
    row!(InsertPara, SelfVerifyFailed, 1),
    row!(DeletePara, StructuralCodec, 2),
    row!(InsertPara, StructuralCodec, 2),
    row!(DeletePara, UpstreamUnmapped, 1),
    row!(InsertPara, UpstreamUnmapped, 1),

    row!(Read, ReadTargetRequired, 1),
    row!(Read, ReadParasWithoutSection, 1),
    row!(Read, ReadParasInvalid, 1),
    row!(Read, DecodeFailed, 2),
    row!(Read, SectionOutOfRange, 1),
    row!(Read, ReadParaRangeInvalid, 1),
    row!(Read, TableNotFound, 1),
    row!(Read, TableGridInvalid, 1),
    row!(Read, FieldNotFound, 1),

    row!(Fields, DecodeFailed, 2),

    row!(Fill, DecodeFailed, 2, ops),
    row!(Fill, NoValues, 1, "예: hwpforge fill doc.hwpx --set 과제명=\"AI 문서 자동화\" -o out.hwpx"),
    row!(Fill, EmptyFieldValue, 1, ops),
    row!(Fill, FieldNameAmbiguous, 1, ops),
    row!(Fill, FieldNotFillable, 1, "병합-run 모호 필드 또는 빈 본문 — 한컴에서 재저장하거나 from-json --base 로 재생성하세요"),
    row!(Fill, FillFailed, 2),

    row!(SetCell, DecodeFailed, 2, ops),
    row!(SetCell, InvalidSetCellArgs, 1),
    row!(SetCell, InvalidSetCellMap, 1),
    row!(SetCell, TableGridInvalid, 1, ops),
    row!(SetCell, CellNotFound, 1),
    row!(SetCell, CellLabelAmbiguous, 1, "라벨이 여러 셀과 일치합니다 — --at 좌표로 직접 지정하세요"),
    row!(SetCell, CellHasNonTextContent, 1, ops),
    row!(SetCell, CellTargetDuplicate, 1),
    row!(SetCell, CellTargetConflict, 1, ops),
    row!(SetCell, InputNotRoundtripSafe, 1, ops),
    row!(SetCell, InputEntriesNotCarried, 1, UNCARRIED_ENTRIES_HINT),
    row!(SetCell, SetCellCodecFailed, 1),
    row!(SetCell, UpstreamUnmapped, 1),

    row!(StampPlan, DecodeFailed, 2, ops),
    row!(StampPlan, StampCodecFailed, 2),

    row!(Stamp, DecodeFailed, 2, ops),
    row!(Stamp, InputNotRoundtripSafe, 1, "이 입력은 무손실 재인코드가 증명되지 않아 스탬핑을 거부합니다 (fail-closed). 코덱 갭 수정 또는 E4 preserve-first 경로가 필요합니다"),
    row!(Stamp, InputEntriesNotCarried, 1, UNCARRIED_ENTRIES_HINT),
    row!(Stamp, StampManifestInvariant, 2),
    row!(Stamp, StampCodecFailed, 2),
    row!(Stamp, StampSourceHashMismatch, 1, "문서가 변경됐습니다 — `stamp-plan` 을 다시 실행해 맵의 source_sha256 을 갱신하세요"),
    row!(Stamp, StampDeltaMismatch, 2, ops),

    row!(Stamp, UpstreamUnmapped, 2),
    row!(Stamp, TableNotFound, 1),
    row!(Stamp, TableGridInvalid, 1),
    row!(Stamp, StampCellNotEmpty, 1, ops),
    row!(Stamp, StampLabelDrift, 1, ops),
    row!(Stamp, StampCellNotCandidate, 1),
    row!(Stamp, StampCellTargetDuplicate, 1),
    row!(Stamp, StampNameEmpty, 1),
    row!(Stamp, StampHintBlank, 1, ops),
    row!(Stamp, StampSpecStale, 1, "문서가 변경됐거나 span 이 어긋났습니다 — `stamp-plan` 을 다시 실행해 맵을 갱신하세요"),
    row!(Stamp, StampMarkerMismatch, 1),
    row!(Stamp, StampSpecDuplicate, 1),

    row!(Patch, JsonParseFailed, 2),
    row!(Patch, GridAddrInvalid, 2, "Grid addresses come from to-json output; after structural edits, drop the stale addr fields (or re-export) and retry"),
    row!(Patch, DecodeFailed, 2),
    row!(Patch, PatchFailed, 2, "Re-export the target section with this version of hwpforge so the JSON contains preservation metadata. Structural or style changes still require a broader rebuild workflow."),
    row!(Patch, UpstreamUnmapped, 2, "Update hwpforge so the CLI understands the newer section workflow error."),

    row!(Templates, PresetNotFound, 1, "Run 'hwpforge templates list' to see available presets"),

    row!(ToMd, DecodeFailed, 2),

    row!(ToMd, ValidationFailed, 2),
    row!(ToMd, EncodeFailed, 2),

    row!(Validate, DecodeFailed, 2),
    row!(AuditHwp5, DecodeFailed, 2),
    row!(AuditHwp5, Hwp5DecodeFailed, 2),
    row!(DeletePara, EncodeFailed, 2),
    row!(DeletePara, EncodeSemanticLoss, 2),
    row!(DeletePara, InternalInvariant, 2),
    row!(DeletePara, ProtectedFieldRange, 2),
    row!(DeletePara, ValidationFailed, 2),
    row!(Fill, EncodeFailed, 2),
    row!(Fill, EncodeSemanticLoss, 2),
    row!(Fill, FieldNotFound, 1),
    row!(Fill, InternalInvariant, 2),
    row!(Fill, PatchFailed, 2),
    row!(Fill, ProtectedFieldRange, 2),
    row!(Fill, SectionOutOfRange, 2),
    row!(Fill, ValidationFailed, 2),
    row!(InsertPara, EncodeFailed, 2),
    row!(InsertPara, EncodeSemanticLoss, 2),
    row!(InsertPara, InternalInvariant, 2),
    row!(InsertPara, ProtectedFieldRange, 2),
    row!(InsertPara, ValidationFailed, 2),
    row!(Inspect, AnalysisFailed, 2),
    row!(Patch, EncodeFailed, 2),
    row!(Patch, EncodeSemanticLoss, 2),
    row!(Patch, InternalInvariant, 2),
    row!(Patch, ProtectedFieldRange, 2),
    row!(Patch, SectionIndexMismatch, 2),
    row!(Patch, SectionOutOfRange, 1),
    row!(Patch, ValidationFailed, 2),
    row!(Schema, UnknownSchemaType, 1),
    row!(SetCell, EncodeFailed, 1),
    row!(SetCell, EncodeSemanticLoss, 1),
    row!(SetCell, InternalInvariant, 1),
    row!(SetCell, SelfVerifyFailed, 1),
    row!(SetCell, TableNotFound, 1),
    row!(SetCell, ValidationFailed, 1),
    row!(Stamp, EncodeFailed, 2),
    row!(Stamp, EncodeSemanticLoss, 2),
    row!(Stamp, InternalInvariant, 2),
    row!(Stamp, InvalidStampMap, 1),
    row!(Stamp, ManifestPathConflict, 1),
    row!(Stamp, SelfVerifyFailed, 2),
    row!(Stamp, StampCandidateUncovered, 1),
    row!(Stamp, StampCellNotAnchor, 1),
    row!(Stamp, StampNameCollision, 1),
    row!(Stamp, StampNameDuplicate, 1),
    row!(Stamp, ValidationFailed, 2),
    row!(ToJson, InvalidExtension, 1),
    row!(ToJson, PatchFailed, 2),
    row!(ToJson, SectionIndexMismatch, 2),
    row!(ToJson, SectionOutOfRange, 1),
    row!(ToJson, UpstreamUnmapped, 2),
    row!(Validate, JsonSerializeFailed, 2),

];

#[must_use]
pub fn cli_error(cmd: Command, err: OpsError) -> CliError {
    let canonical = err.code();
    if let OpsError::EncodeSemanticLoss { warnings, .. } = &err {
        if let Some(info) = semantic_loss_error(cmd, warnings) {
            return info;
        }
    }

    match &err {
        OpsError::Fill(FillError::UnknownField { available, .. }) if cmd == Command::Fill => {
            return CliError::new(CliCode::Ops(canonical), err.to_string()).with_hint(format!(
                "사용 가능한 필드: [{}] — `hwpforge fields <file>` 로 확인",
                available.join(", ")
            ));
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionOutOfRange { sections, .. })
            if cmd == Command::ToJson || cmd == Command::Patch =>
        {
            return CliError::new(CliCode::Ops(canonical), err.to_string())
                .with_hint(format!("Valid range: 0..{}", sections.saturating_sub(1)));
        }
        OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
            requested,
            actual,
        }) if cmd == Command::Patch => {
            return CliError::new(CliCode::Ops(canonical), err.to_string()).with_hint(format!(
                "Use --section {actual} to match the JSON, or re-export section {requested} with this version of hwpforge."
            ));
        }

        OpsError::Stamper(StamperError::CellStamp(CellStampError::NotAnAnchor {
            anchor, ..
        })) => {
            let mut e = CliError::new(CliCode::Ops(canonical), err.to_string());
            if let Some(anchor) = anchor {
                e = e.with_hint(format!(
                    "이 좌표는 병합 피복 위치입니다 — anchor ({},{}) 를 지정하세요",
                    anchor.row, anchor.col
                ));
            }
            return e;
        }

        OpsError::Stamper(StamperError::Stamp(StampError::DuplicateName { .. })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string());
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::DuplicateName { .. })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string());
        }
        OpsError::Stamper(StamperError::Stamp(StampError::NameCollision { .. })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string())
                .with_hint("기존 누름틀과 이름이 겹칩니다 — `fields` 로 기존 이름을 확인하세요");
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::NameCollision { .. })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string());
        }
        OpsError::Stamper(StamperError::Stamp(StampError::UncoveredCandidate { .. })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string()).with_hint(
                "모든 무가드 후보는 이름을 붙이거나 ignore 로 명시해야 합니다 — `stamp-plan` 출력을 빠짐없이 분류하세요",
            );
        }
        OpsError::Stamper(StamperError::CellStamp(CellStampError::UncoveredCandidate {
            ..
        })) => {
            return CliError::new(CliCode::Ops(canonical), err.to_string())
                .with_hint("모든 무가드 셀 후보는 이름을 붙이거나 ignore 로 명시해야 합니다");
        }
        OpsError::CellEdit(CellEditError::TableNotFound { tables, .. })
            if cmd == Command::SetCell =>
        {
            return CliError::new(CliCode::Ops(canonical), err.to_string()).with_hint(format!(
                "문서에 표가 {tables}개 있습니다 (to-json export 순서 기준 0-base)"
            ));
        }
        _ => {}
    }

    let code = err.code();
    let message = cli_message(cmd, code, err.to_string());
    let hint = match lookup(cmd, |row| row.code == CliCode::Ops(code)) {
        Some(row) => resolved_hint(row.hint, err.hint()),

        None => err.hint(),
    };
    match hint {
        Some(hint) => CliError::new(CliCode::Ops(code), message).with_hint(hint),
        None => CliError::new(CliCode::Ops(code), message),
    }
}

#[must_use]
fn cli_message(cmd: Command, code: OpsCode, message: String) -> String {
    match (cmd, code) {
        (Command::Read, OpsCode::ReadParasInvalid) => {
            match message.split_once("Cannot parse paras ") {
                Some((head, spec)) => format!("{head}Cannot parse --paras {spec}"),
                None => message,
            }
        }
        _ => message,
    }
}

fn semantic_loss_error(cmd: Command, warnings: &[WarningInfo]) -> Option<CliError> {
    match cmd {
        Command::SetCell => {
            let message = match warnings.first() {
                Some(w) => format!(
                    "codec failure: encode produced a semantic-loss warning (fail-closed): {}",
                    w.message
                ),
                None => {
                    "codec failure: encode produced a semantic-loss warning (fail-closed)".into()
                }
            };
            Some(CliError::new(CliCode::Ops(OpsCode::EncodeSemanticLoss), message))
        }
        Command::Stamp => {
            let message = match warnings.first() {
                Some(w) => {
                    format!("encode produced a semantic-loss warning (fail-closed): {}", w.message)
                }
                None => "encode produced a semantic-loss warning (fail-closed)".into(),
            };
            Some(CliError::new(CliCode::Ops(OpsCode::EncodeSemanticLoss), message))
        }
        _ => None,
    }
}

#[must_use]
pub fn convert_error(cmd: Command, err: ConvertOpsError) -> CliError {
    let code = err.code();
    let message = err.to_string();
    let mut hint = match lookup(cmd, |row| row.code == CliCode::Ops(code)) {
        Some(row) => resolved_hint(row.hint, err.hint()),
        None => err.hint(),
    };

    // 이것을 실패시키는 것: 이 분기 삭제
    if cmd == Command::ToPdf && pdf_cause_has_its_own_hint(err.cause()) {
        hint = err.hint();
    }
    let base = match hint {
        Some(hint) => CliError::new(CliCode::Ops(code), message).with_hint(hint),
        None => CliError::new(CliCode::Ops(code), message),
    };
    match err.cause_info() {
        Some(cause) => base.with_cause(ErrorCause {
            stage: cause.stage,
            code: cause.code.as_str(),
            kind: cause.kind,
            location: cause.location,
        }),
        None => base,
    }
}

fn pdf_cause_has_its_own_hint(cause: Option<hwpforge_smithy_pdf::PdfErrorCode>) -> bool {
    use hwpforge_smithy_pdf::PdfErrorCode as C;
    matches!(
        cause,
        Some(
            C::MissingLayoutCache | C::NoRenderableCache | C::InvalidCache | C::UnsupportedContent
        )
    )
}

#[must_use]
pub fn exit_code(cmd: Command, code: CliCode) -> i32 {
    registered_exit(cmd, code).unwrap_or(2)
}

fn registered_exit(cmd: Command, code: CliCode) -> Option<i32> {
    match code {
        CliCode::Ops(OpsCode::InputTooLarge) => Some(1),
        CliCode::Ops(OpsCode::FileNotFound | OpsCode::FileReadFailed) => Some(match cmd {
            Command::ToMd | Command::ConvertHwp5 => 2,
            _ => 1,
        }),
        CliCode::Ops(OpsCode::FileWriteFailed) => Some(match cmd {
            Command::DeletePara | Command::InsertPara | Command::ConvertHwp5 => 2,
            _ => 1,
        }),
        CliCode::Cli(CliOnly::InvalidSet | CliOnly::DuplicateSet) => Some(1),
        CliCode::Cli(
            CliOnly::Hwp5CensusFailed
            | CliOnly::Hwp5SemanticFailed
            | CliOnly::HwpxAnalysisFailed
            | CliOnly::HwpxCensusFailed,
        ) => Some(2),
        _ => lookup(cmd, |row| row.code == code).map(|row| row.exit),
    }
}

pub fn exit_ops_error(cmd: Command, err: OpsError, json_mode: bool) -> ! {
    exit_ops_error_with_hint(cmd, err, None, json_mode)
}

pub fn exit_ops_error_with_hint(
    cmd: Command,
    err: OpsError,
    context_hint: Option<&'static str>,
    json_mode: bool,
) -> ! {
    let mut ce = cli_error(cmd, err);
    if ce.hint.is_none() {
        if let Some(hint) = context_hint {
            ce = ce.with_hint(hint);
        }
    }
    ce.exit(json_mode, cmd);
}

pub fn exit_convert_error(cmd: Command, err: ConvertOpsError, json_mode: bool) -> ! {
    let ce = convert_error(cmd, err);
    ce.exit(json_mode, cmd);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_code(cmd: Command, err: OpsError, expected_code: &str, expected_exit: i32) {
        let got = cli_error(cmd, err);
        assert_eq!(got.code.as_str(), expected_code, "{cmd:?}: {got:?}");
        assert_eq!(exit_code(cmd, got.code), expected_exit, "{cmd:?}: {got:?}");
    }

    #[test]
    fn from_ops_rows_keep_emitting_the_same_hint_text_as_before_the_refactor() {
        let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("bad".into()));
        let got = cli_error(Command::Inspect, err);
        assert_eq!(got.code.as_str(), "DECODE_FAILED");
        assert_eq!(got.hint.as_deref(), Some("Check that the file is a valid HWPX document"));

        let err = OpsError::Fill(FillError::EmptyValue { name: "x".into() });
        let got = cli_error(Command::Fill, err);
        assert_eq!(got.code.as_str(), "EMPTY_FIELD_VALUE");
        assert_eq!(
            got.hint.as_deref(),
            Some("빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요")
        );
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn preset_not_found_code_is_shared_by_commands() {
        assert_code(
            Command::Convert,
            OpsError::PresetNotFound { name: "gov".into() },
            "PRESET_NOT_FOUND",
            1,
        );
        assert_code(
            Command::Templates,
            OpsError::PresetNotFound { name: "gov".into() },
            "PRESET_NOT_FOUND",
            1,
        );
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn to_md_validation_failed_uses_canonical_spelling() {
        use hwpforge_core::CoreError;
        let err =
            OpsError::Core(CoreError::InvalidStructure { context: "x".into(), reason: "y".into() });
        assert_code(Command::ToMd, err, "VALIDATION_FAILED", 2);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn set_cell_semantic_loss_reconstructs_the_legacy_codec_message() {
        let err = OpsError::EncodeSemanticLoss {
            warnings: vec![WarningInfo::new("NOTE_HEAD_SKIPPED", "titleMark dropped")],
            others: Vec::new(),
        };
        let got = cli_error(Command::SetCell, err);
        assert_eq!(got.code.as_str(), "ENCODE_SEMANTIC_LOSS");
        assert_eq!(
            got.message,
            "codec failure: encode produced a semantic-loss warning (fail-closed): titleMark dropped"
        );
        assert_eq!(exit_code(Command::SetCell, got.code), 1);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn stamp_semantic_loss_reconstructs_the_legacy_message_and_exit_2() {
        let err = OpsError::EncodeSemanticLoss {
            warnings: vec![WarningInfo::new("NOTE_HEAD_SKIPPED", "titleMark dropped")],
            others: Vec::new(),
        };
        let got = cli_error(Command::Stamp, err);
        assert_eq!(got.code.as_str(), "ENCODE_SEMANTIC_LOSS");
        assert_eq!(
            got.message,
            "encode produced a semantic-loss warning (fail-closed): titleMark dropped"
        );
        assert_eq!(exit_code(Command::Stamp, got.code), 2);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn stamp_upstream_unmapped_uses_canonical_spelling() {
        let err = OpsError::Rejected {
            code: OpsCode::UpstreamUnmapped,
            reason: "unrecognized upstream stamper variant".into(),
        };
        assert_code(Command::Stamp, err, "UPSTREAM_UNMAPPED", 2);
    }

    #[test]
    fn fill_field_not_found_carries_the_available_list_hint() {
        let err = OpsError::Fill(FillError::UnknownField {
            name: "x".into(),
            available: vec!["a".into(), "b".into()],
        });
        let got = cli_error(Command::Fill, err);
        assert_eq!(got.code.as_str(), "FIELD_NOT_FOUND");
        assert!(got.hint.as_deref().unwrap_or_default().contains("[a, b]"), "{got:?}");
    }

    #[test]
    fn section_index_mismatch_hint_differs_between_to_json_and_patch() {
        let err = || {
            OpsError::SectionWorkflow(SectionWorkflowError::SectionIndexMismatch {
                requested: 1,
                actual: 2,
            })
        };
        let to_json = cli_error(Command::ToJson, err());
        assert_eq!(to_json.code.as_str(), "SECTION_INDEX_MISMATCH");
        assert!(to_json.hint.is_none(), "{to_json:?}");

        let patch = cli_error(Command::Patch, err());
        assert_eq!(patch.code.as_str(), "SECTION_INDEX_MISMATCH");
        assert!(patch.hint.as_deref().unwrap_or_default().contains("--section 2"), "{patch:?}");
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn to_pdf_preserves_hwp5_decode_code() {
        use hwpforge_convert::ConvertError;
        use hwpforge_smithy_hwp5::Hwp5Error;

        let decode = ConvertOpsError::Convert(ConvertError::Decode(Hwp5Error::NotHwp5 {
            detail: "x".into(),
        }));
        let convert_error_out = convert_error(Command::ToPdf, decode);
        assert_eq!(convert_error_out.code.as_str(), "HWP5_DECODE_FAILED");
        assert_eq!(exit_code(Command::ToPdf, convert_error_out.code), 2);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn convert_hwp5_preserves_ops_decode_code() {
        use hwpforge_convert::ConvertError;
        use hwpforge_smithy_hwp5::Hwp5Error;

        let decode = ConvertOpsError::Convert(ConvertError::Decode(Hwp5Error::NotHwp5 {
            detail: "x".into(),
        }));
        let got = convert_error(Command::ConvertHwp5, decode);
        assert_eq!(got.code.as_str(), "HWP5_DECODE_FAILED");
        assert_eq!(
            got.hint.as_deref(),
            Some("Check that the source is a supported HWP5 document and the output path is writable")
        );
        assert_eq!(exit_code(Command::ConvertHwp5, got.code), 2);
    }

    #[test]
    fn to_pdf_unrecognized_format_uses_its_own_legacy_hint() {
        let got = convert_error(Command::ToPdf, ConvertOpsError::UnrecognizedFormat);
        assert_eq!(got.code.as_str(), "UNRECOGNIZED_FORMAT");
        assert_eq!(
            got.hint.as_deref(),
            Some("to-pdf detects the format by content — the extension is only a hint")
        );
    }

    #[test]
    fn validate_decode_failed_has_no_hint_and_exits_2() {
        let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("not a zip file".into()));
        assert_code(Command::Validate, err, "DECODE_FAILED", 2);
    }

    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn structural_commands_decode_failed_output_is_unchanged_by_their_new_table_rows() {
        for cmd in [
            Command::DeletePara,
            Command::InsertPara,
            Command::Fill,
            Command::SetCell,
            Command::StampPlan,
            Command::Stamp,
        ] {
            let err = OpsError::decode(hwpforge_smithy_hwpx::HwpxError::Zip("bad".into()));
            let got = cli_error(cmd, err);
            assert_eq!(got.code.as_str(), "DECODE_FAILED", "{cmd:?}");
            assert_eq!(
                got.hint.as_deref(),
                Some("Check that the file is a valid HWPX document"),
                "{cmd:?}"
            );
            assert_eq!(exit_code(cmd, got.code), 2, "{cmd:?}");
        }
    }

    #[test]
    // 이것을 실패시키는 것: 코드 출력·종료 정책·등록을 변경, 미소비 기준선 또는 정책 행을 추가.
    fn w1_manifest_canonical_adapter() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../hwpforge/tests/data/error_contract_018.json"))
                .unwrap();
        let baseline: std::collections::BTreeSet<_> = include_str!("../tests/data/codes-0.18.txt")
            .lines()
            .filter(|line| !line.starts_with('#'))
            .map(|line| line.rsplit_once('\t').unwrap().0.to_owned())
            .collect();
        let mut consumed = std::collections::BTreeSet::new();
        let mut covered = Vec::new();
        for case in manifest["cases"].as_array().unwrap() {
            let Some(surface) = case["surfaces"].get("cli") else { continue };
            let command = match surface["operation"].as_str().unwrap() {
                "audit-hwp5" => Command::AuditHwp5,
                "census-hwp5" => Command::CensusHwp5,
                "convert-hwp5" => Command::ConvertHwp5,
                "to-pdf" => Command::ToPdf,
                "convert" => Command::Convert,
                "inspect" => Command::Inspect,
                "to-json" => Command::ToJson,
                "from-json" => Command::FromJson,
                "outline" => Command::Outline,
                "diff" => Command::Diff,
                "delete-para" => Command::DeletePara,
                "insert-para" => Command::InsertPara,
                "read" => Command::Read,
                "fields" => Command::Fields,
                "fill" => Command::Fill,
                "set-cell" => Command::SetCell,
                "stamp-plan" => Command::StampPlan,
                "stamp" => Command::Stamp,
                "patch" => Command::Patch,
                "templates" => Command::Templates,
                "schema" => Command::Schema,
                "to-md" => Command::ToMd,
                "validate" => Command::Validate,
                other => panic!("unconsumed cli operation: {other}"),
            };
            let expected = case["expected_code"].as_str().unwrap();
            consumed.insert(format!(
                "{}\t{}\t{}",
                surface["operation"].as_str().unwrap(),
                expected,
                case["id"].as_str().unwrap()
            ));
            let code = match expected {
                "INVALID_SET" => CliCode::Cli(CliOnly::InvalidSet),
                "DUPLICATE_SET" => CliCode::Cli(CliOnly::DuplicateSet),
                "HWP5_CENSUS_FAILED" => CliCode::Cli(CliOnly::Hwp5CensusFailed),
                "HWP5_SEMANTIC_FAILED" => CliCode::Cli(CliOnly::Hwp5SemanticFailed),
                "HWPX_ANALYSIS_FAILED" => CliCode::Cli(CliOnly::HwpxAnalysisFailed),
                "HWPX_CENSUS_FAILED" => CliCode::Cli(CliOnly::HwpxCensusFailed),
                other => CliCode::Ops(OpsCode::lookup(other).unwrap()),
            };
            covered.push((command, code));
            let got = match code {
                CliCode::Ops(code) => cli_error(
                    command,
                    OpsError::Rejected { code, reason: "injected adapter boundary".into() },
                ),
                CliCode::Cli(_) => CliError::new(code, "CLI argument or diagnostic rejection"),
            };
            assert_eq!(
                registered_exit(command, code),
                Some(surface["exit"].as_i64().unwrap() as i32),
                "{}",
                case["id"]
            );
            assert_eq!(got.code.as_str(), expected, "{}", case["id"]);
            let baseline_row = include_str!("../tests/data/codes-0.18.txt")
                .lines()
                .find(|line| line.split('\t').any(|field| field == case["id"].as_str().unwrap()))
                .unwrap();
            let hint: Option<String> =
                serde_json::from_str(baseline_row.rsplit_once('\t').unwrap().1).unwrap();
            match hint.as_deref() {
                Some("@dynamic" | "@ops") => {}
                expected => assert_eq!(got.hint.as_deref(), expected, "{}", case["id"]),
            }

            assert_eq!(
                exit_code(command, got.code),
                surface["exit"].as_i64().unwrap() as i32,
                "{}",
                case["id"]
            );
        }
        for row in TABLE {
            assert!(
                covered.contains(&(row.cmd, row.code)),
                "uncovered policy row: {:?}/{:?}",
                row.cmd,
                row.code
            );
        }
        assert_eq!(consumed, baseline, "baseline rows must link to consumed manifest cases");
    }
    #[test]
    // 이것을 실패시키는 것: canonical 출력 코드를 MUTANT로 치환.
    fn to_pdf_hint_follows_the_render_cause() {
        use hwpforge_smithy_pdf::PdfError;
        let hint_of = |e: PdfError| {
            let err = convert_error(Command::ToPdf, ConvertOpsError::Pdf(e));
            assert_eq!(err.code.as_str(), "PDF_RENDER_FAILED");
            assert!(err.cause.is_some(), "the cause block stays");
            err.hint.expect("a hint")
        };
        let frozen = "cacheless documents need a Hancom re-save; font errors may need --font-dir/--discovery or --degraded".to_string();
        let missing = hint_of(PdfError::MissingLayoutCache { count: 1, first: "s0/p0".into() });
        assert!(missing.contains("re-saving the document in Hancom writes one"), "{missing}");
        let none = hint_of(PdfError::NoRenderableCache { section: 0 });
        assert_eq!(none, missing);
        let invalid = hint_of(PdfError::InvalidCache { detail: "textpos not monotonic".into() });
        assert!(invalid.contains("re-saving in Hancom rewrites it"), "{invalid}");
        let unsupported = hint_of(PdfError::UnsupportedContent {
            kind: "non-default table position",
            location: "s0/p0".into(),
        });
        assert!(unsupported.contains("export the PDF from Hancom"), "{unsupported}");
        assert!(unsupported.contains("does not change that"), "{unsupported}");
        let font = hint_of(PdfError::FontUnresolved { face: "함초롬바탕".into() });
        assert_eq!(font, frozen);
        for hint in [&missing, &invalid, &unsupported] {
            assert_ne!(hint, &frozen);
        }
        assert_eq!(
            exit_code(
                Command::ToPdf,
                convert_error(
                    Command::ToPdf,
                    ConvertOpsError::Pdf(PdfError::UnsupportedContent {
                        kind: "x",
                        location: String::new()
                    })
                )
                .code
            ),
            2
        );
    }
}
