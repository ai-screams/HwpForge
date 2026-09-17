//! Shared operation layer — one implementation of each document operation,
//! called by every frontend (CLI, MCP server, Python bindings).
//!
//! # What lives here
//!
//! One free function per operation. The input is bytes (`&[u8]` for HWPX) or
//! text (`&str` for Markdown and JSON), options are a [`Default`] struct with
//! consuming `with_*` builders, and the output is
//! `Result<XxxOutput, OpsError>`. **Operation functions never touch the
//! filesystem**; the one place that reads files is the `fs` submodule
//! (feature `ops-md`), and it only resolves assets a caller already planned.
//!
//! Output structs are `#[non_exhaustive]` and do **not** derive serde: the
//! serialisable payload is the wire DTO they carry, and warnings become
//! [`WarningInfo`] through [`OpsWarning::info`].
//!
//! # Errors and codes
//!
//! [`OpsError`] wraps the library errors an operation can meet without
//! normalising them, and [`OpsError::code`] classifies each one as an
//! [`OpsCode`] — the single stable code table, shared with the other
//! frontends through `hwpforge_foundation::diagnostics`.
//!
//! Upstream enums that are exhaustive from here (for example [`FillError`])
//! are matched without a wildcard so the compiler catches a new variant.
//! Upstream enums that are `#[non_exhaustive]` force a wildcard, so the
//! classification goes through the defining crate's own stable method first
//! ([`HwpxError::code`], `MdError::code`) and only what is left falls
//! through to [`OpsCode::UpstreamUnmapped`], which keeps the original type
//! name and message. `tests/ops_error_inventory.rs` fails as
//! soon as an inventoried variant has no explicit arm.
//!
//! # Features
//!
//! `ops-hwpx` gives the HWPX-only operations, `ops-md` adds the Markdown
//! ones, and `ops` is the alias for everything.

#[cfg(feature = "ops-md")]
pub mod fs;
pub mod inspect;

use hwpforge_core::CoreError;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_smithy_hwpx::{
    grid_addr::GridAddrError,
    stamp::{CellStampError, StampError, StampMapError, StamperError},
    CellEditError, DecodeWarning, EncodeOutcome, EncodeWarning, FillError, HwpxError,
    HwpxErrorCode, ReadError, SectionWorkflowError, StructuralEditError,
};

#[cfg(feature = "ops-md")]
use hwpforge_smithy_md::{assets::AssetOutcome, MdError, MdErrorCode, MdWarning};

pub use inspect::{inspect, InspectOptions, InspectOutput, InspectReport};

// ── errors ──────────────────────────────────────────────────────

/// Everything an operation in this module can fail with.
///
/// Library errors are wrapped, not normalised: the original error keeps its
/// own message and source chain, and [`OpsError::code`] adds the stable
/// classification that frontends report.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OpsError {
    /// HWPX codec failure (decode, encode, packaging).
    #[error(transparent)]
    Hwpx(#[from] HwpxError),

    /// Markdown codec failure.
    #[cfg(feature = "ops-md")]
    #[error(transparent)]
    Md(#[from] MdError),

    /// Click-here field fill failure.
    #[error(transparent)]
    Fill(#[from] FillError),

    /// Table cell edit failure.
    #[error(transparent)]
    CellEdit(#[from] CellEditError),

    /// Read/projection failure (`outline`, `read`, `fields`).
    #[error(transparent)]
    Read(#[from] ReadError),

    /// Paragraph insert/delete failure.
    #[error(transparent)]
    StructuralEdit(#[from] StructuralEditError),

    /// Cell grid address check failure on an exported JSON tree.
    #[error(transparent)]
    GridAddr(#[from] GridAddrError),

    /// Section export/patch workflow failure.
    #[error(transparent)]
    SectionWorkflow(#[from] SectionWorkflowError),

    /// Template stamping failure.
    #[error(transparent)]
    Stamper(#[from] StamperError),

    /// Stamp request (map) parse or validation failure.
    #[error(transparent)]
    StampMap(#[from] StampMapError),

    /// Core document failure, including validation.
    #[error(transparent)]
    Core(#[from] CoreError),

    /// JSON input could not be parsed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The requested style preset does not exist.
    #[error("preset not found: {name}")]
    PresetNotFound {
        /// The preset name that was requested.
        name: String,
    },

    /// Arguments were syntactically valid but semantically unusable.
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// What made the arguments unusable.
        reason: String,
    },

    /// The document declares no fonts, so a restyle has nothing to rebind.
    #[error("document has no fonts to restyle")]
    NoFonts,

    /// A regenerating edit refused to emit bytes because encoding reported
    /// semantic-loss warnings (fail-closed — see [`take_bytes_fail_closed`]).
    #[error("encode produced semantic-loss warnings (fail-closed)")]
    EncodeSemanticLoss {
        /// The semantic-loss warnings that caused the refusal.
        warnings: Vec<WarningInfo>,
        /// The remaining, non-semantic warnings of the same encode.
        others: Vec<WarningInfo>,
    },
}

impl OpsError {
    /// The stable code a frontend reports for this failure.
    ///
    /// See the module docs for how exhaustive and `#[non_exhaustive]`
    /// upstream enums are treated differently.
    #[must_use]
    pub fn code(&self) -> OpsCode {
        match self {
            Self::Hwpx(e) => hwpx_code(e),
            // `md_code` documents why the two asset-contract variants are
            // `INVALID_INPUT` rather than an internal invariant.
            #[cfg(feature = "ops-md")]
            Self::Md(e) => md_code(e),
            Self::Fill(e) => fill_code(e),
            Self::CellEdit(e) => cell_edit_code(e),
            Self::Read(e) => read_code(e),
            Self::StructuralEdit(e) => structural_code(e),
            Self::GridAddr(e) => grid_addr_code(e),
            Self::SectionWorkflow(e) => section_workflow_code(e),
            Self::Stamper(e) => stamper_code(e),
            Self::StampMap(e) => stamp_map_code(e),
            Self::Core(e) => core_code(e),
            Self::Json(_) => OpsCode::JsonParseFailed,
            Self::PresetNotFound { .. } => OpsCode::PresetNotFound,
            Self::InvalidInput { .. } => OpsCode::InvalidInput,
            Self::NoFonts => OpsCode::NoFonts,
            Self::EncodeSemanticLoss { .. } => OpsCode::EncodeSemanticLoss,
        }
    }

    /// A static recovery suggestion for this failure class, if there is one.
    ///
    /// The hints reproduce what the CLI prints today. Hints that quote a
    /// value (the list of available field names, for example) are not
    /// static, so they stay with the frontend that formats them.
    #[must_use]
    pub fn hint(&self) -> Option<&'static str> {
        hint_for(self.code())
    }
}

fn hint_for(code: OpsCode) -> Option<&'static str> {
    Some(match code {
        OpsCode::DecodeFailed => "Check that the file is a valid HWPX document",
        OpsCode::EmptyFieldValue => {
            "빈 값 채우기는 미지원 — 값을 지우려면 한컴에서 편집하세요"
        }
        OpsCode::FieldNameAmbiguous => {
            "같은 이름의 누름틀이 여러 개라 대상이 모호합니다 — 문서에서 이름을 유일하게 하세요"
        }
        OpsCode::FieldNotFillable => {
            "병합-run 모호 필드 또는 빈 본문 — 한컴에서 재저장하거나 from-json --base 로 재생성하세요"
        }
        OpsCode::PresetNotFound => "`templates` 로 사용 가능한 프리셋을 확인하세요",
        OpsCode::NoFonts => "문서에 글꼴 정의가 없습니다 — `validate` 로 구조를 확인하세요",
        OpsCode::TableGridInvalid => {
            "이 표는 셀 span 이 well-formed 격자를 이루지 않아 주소 지정이 불가합니다"
        }
        OpsCode::CellLabelAmbiguous => {
            "라벨이 여러 셀과 일치합니다 — 좌표(at)로 직접 지정하세요"
        }
        OpsCode::CellHasNonTextContent => {
            "표/이미지/컨트롤이 든 셀은 파괴 방지를 위해 교체를 거부합니다"
        }
        OpsCode::CellTargetConflict => {
            "바깥 셀 교체가 다른 편집이 노리는 중첩 표를 파괴합니다"
        }
        OpsCode::InputNotRoundtripSafe => {
            "이 입력은 무손실 재인코드가 증명되지 않아 편집을 거부합니다 (fail-closed)"
        }
        OpsCode::InputEntriesNotCarried => {
            "재인코드 시 유실될 ZIP 엔트리가 있어 거부합니다 (fail-closed)"
        }
        OpsCode::StampSourceHashMismatch | OpsCode::StampLabelDrift | OpsCode::StampSpecStale => {
            "문서가 변경됐습니다 — `stamp-plan` 을 다시 실행해 맵을 갱신하세요"
        }
        OpsCode::StampCellNotEmpty => "클래스-B 대상은 whitespace-only 빈 셀이어야 합니다",
        OpsCode::StampHintBlank => {
            "빈 셀엔 마커가 없어 hint 가 필수입니다 (plan 의 suggested_hint 참고)"
        }
        OpsCode::StampCandidateUncovered => {
            "모든 무가드 후보는 이름을 붙이거나 ignore 로 명시해야 합니다"
        }
        OpsCode::StampDeltaMismatch | OpsCode::SelfVerifyFailed => {
            "산출물 검증 실패 — 코덱 버그 가능성이 있어 무출력으로 거부했습니다"
        }
        OpsCode::EncodeSemanticLoss => {
            "재인코드가 의미를 잃어 바이트를 내지 않았습니다 (fail-closed) — 경고 목록을 확인하세요"
        }
        OpsCode::UpstreamUnmapped => {
            "이 버전의 hwpforge 가 모르는 상류 오류입니다 — 업그레이드하거나 이슈로 보고하세요"
        }
        _ => return None,
    })
}

// ── per-enum classification ─────────────────────────────────────
//
// One function per wrapped enum. `tests/ops_error_inventory.rs` holds the
// same tables as literal data and fails when an upstream variant is missing.

fn hwpx_code(error: &HwpxError) -> OpsCode {
    // Payload-bearing delegation first: a Core failure keeps the code it
    // would have had on its own, whichever layer wrapped it.
    if let HwpxError::Core(inner) = error {
        return core_code(inner);
    }
    match error.code() {
        HwpxErrorCode::Zip
        | HwpxErrorCode::InvalidMimetype
        | HwpxErrorCode::MissingFile
        | HwpxErrorCode::XmlParse
        | HwpxErrorCode::InvalidAttribute
        | HwpxErrorCode::IndexOutOfBounds
        | HwpxErrorCode::InvalidStructure => OpsCode::DecodeFailed,
        HwpxErrorCode::LayoutCacheDropped | HwpxErrorCode::XmlSerialize => OpsCode::EncodeFailed,
        HwpxErrorCode::Io => OpsCode::IoFailed,
        HwpxErrorCode::Foundation => OpsCode::InternalInvariant,
        // `Core` is handled above; anything else is newer than this table.
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn core_code(error: &CoreError) -> OpsCode {
    match error {
        CoreError::Validation(_) | CoreError::InvalidStructure { .. } => OpsCode::ValidationFailed,
        CoreError::Foundation(_) => OpsCode::InternalInvariant,
        _ => OpsCode::UpstreamUnmapped,
    }
}

/// Classifies a Markdown codec failure.
///
/// Every variant the CLI can already produce keeps the code the CLI prints
/// (`MD_DECODE_FAILED` for the decode family, `INPUT_TOO_LARGE` for an
/// oversized input).
///
/// The two asset-contract variants get dedicated codes.
/// [`MdError::AssetPlanMismatch`] and [`MdError::AssetIdentityConflict`] both
/// mean that the assets a caller provisioned do not line up with the plan the
/// document produced: a reply for an occurrence that was never planned, or two
/// assets claiming one identity with different bytes. They are caller contract
/// violations, but collapsing them into `INVALID_INPUT` would hide *which*
/// contract broke, so they map to [`OpsCode::AssetPlanMismatch`] and
/// [`OpsCode::AssetIdentityConflict`].
/// `tests/ops_error_inventory.rs::asset_contract_violations_get_dedicated_codes`
/// pins it (W1a adversarial review F6).
#[cfg(feature = "ops-md")]
fn md_code(error: &MdError) -> OpsCode {
    if let MdError::Core(inner) = error {
        return core_code(inner);
    }
    match error.code() {
        MdErrorCode::InvalidFrontmatter
        | MdErrorCode::FrontmatterUnclosed
        | MdErrorCode::TemplateResolution
        | MdErrorCode::UnsupportedStructure
        | MdErrorCode::OrphanNoteDefinition
        | MdErrorCode::DuplicateNoteDefinition
        | MdErrorCode::EmptyNoteDefinition
        | MdErrorCode::NestedNoteReference
        | MdErrorCode::NoteExpansionBudgetExceeded
        | MdErrorCode::LosslessParse
        | MdErrorCode::LosslessMissingAttribute
        | MdErrorCode::LosslessInvalidAttribute
        | MdErrorCode::Blueprint => OpsCode::MdDecodeFailed,
        MdErrorCode::FileTooLarge => OpsCode::InputTooLarge,
        MdErrorCode::AssetPlanMismatch => OpsCode::AssetPlanMismatch,
        MdErrorCode::AssetIdentityConflict => OpsCode::AssetIdentityConflict,
        MdErrorCode::Io => OpsCode::IoFailed,
        MdErrorCode::Foundation => OpsCode::InternalInvariant,
        // `Core` is handled above; anything else is newer than this table.
        _ => OpsCode::UpstreamUnmapped,
    }
}

// `FillError` is exhaustive from here — no wildcard, so a new variant is a
// compile error rather than an `UPSTREAM_UNMAPPED` at runtime.
fn fill_code(error: &FillError) -> OpsCode {
    match error {
        FillError::EmptyValue { .. } => OpsCode::EmptyFieldValue,
        FillError::UnknownField { .. } => OpsCode::FieldNotFound,
        FillError::DuplicateFieldName { .. } => OpsCode::FieldNameAmbiguous,
        FillError::UnfillableField { .. } => OpsCode::FieldNotFillable,
        FillError::Workflow(_) => OpsCode::FillFailed,
    }
}

fn cell_edit_code(error: &CellEditError) -> OpsCode {
    match error {
        CellEditError::TableNotFound { .. } => OpsCode::TableNotFound,
        CellEditError::TableGridInvalid { .. } => OpsCode::TableGridInvalid,
        CellEditError::CellNotFound { .. } => OpsCode::CellNotFound,
        CellEditError::LabelAmbiguous { .. } => OpsCode::CellLabelAmbiguous,
        CellEditError::NonTextContent { .. } => OpsCode::CellHasNonTextContent,
        CellEditError::TargetDuplicate { .. } => OpsCode::CellTargetDuplicate,
        CellEditError::TargetConflict { .. } => OpsCode::CellTargetConflict,
        CellEditError::NotRoundTripSafe { .. } => OpsCode::InputNotRoundtripSafe,
        CellEditError::UncarriedZipEntries { .. } => OpsCode::InputEntriesNotCarried,
        CellEditError::Codec(_) => OpsCode::SetCellCodecFailed,
        CellEditError::SemanticLoss { .. } => OpsCode::EncodeSemanticLoss,
        _ => OpsCode::UpstreamUnmapped,
    }
}

// `ReadError` is exhaustive from here — no wildcard.
fn read_code(error: &ReadError) -> OpsCode {
    match error {
        ReadError::Codec(_) => OpsCode::DecodeFailed,
        ReadError::SectionOutOfRange { .. } => OpsCode::ReadSectionOutOfRange,
        ReadError::ParaRangeInvalid { .. } => OpsCode::ReadParaRangeInvalid,
        ReadError::TableOutOfRange { .. } => OpsCode::ReadTableOutOfRange,
        ReadError::TableUnaddressable { .. } => OpsCode::TableGridInvalid,
        ReadError::FieldNotFound { .. } => OpsCode::ReadFieldNotFound,
    }
}

fn structural_code(error: &StructuralEditError) -> OpsCode {
    match error {
        StructuralEditError::Codec(_) => OpsCode::StructuralCodec,
        StructuralEditError::NotRoundTripSafe { .. } => OpsCode::InputNotRoundtripSafe,
        StructuralEditError::UncarriedZipEntries { .. } => OpsCode::InputEntriesNotCarried,
        StructuralEditError::SectionOutOfRange { .. } => OpsCode::SectionOutOfRange,
        StructuralEditError::ParagraphOutOfRange { .. } => OpsCode::ParagraphOutOfRange,
        StructuralEditError::DuplicateTarget { .. } => OpsCode::DuplicateTarget,
        StructuralEditError::ReferenceStranded { .. } => OpsCode::ReferenceStranded,
        StructuralEditError::HardBreakLoss { .. } => OpsCode::HardBreakLoss,
        StructuralEditError::EmptySection { .. } => OpsCode::EmptySection,
        StructuralEditError::SectionPropertiesParagraph { .. } => {
            OpsCode::SectionPropertiesParagraph
        }
        StructuralEditError::SpanCountMismatch { .. } => OpsCode::SpanCountMismatch,
        StructuralEditError::DeltaMismatch { .. } => OpsCode::SelfVerifyFailed,
        StructuralEditError::MultiParagraphText => OpsCode::MultiParagraphText,
        StructuralEditError::InsertBeforeSectionProperties { .. } => {
            OpsCode::InsertBeforeSectionProperties
        }
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn grid_addr_code(error: &GridAddrError) -> OpsCode {
    match error {
        GridAddrError::ShapeMismatch { .. }
        | GridAddrError::AddrMalformed { .. }
        | GridAddrError::AddrMismatch { .. }
        | GridAddrError::AddrOnUnaddressableTable { .. } => OpsCode::GridAddrInvalid,
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn section_workflow_code(error: &SectionWorkflowError) -> OpsCode {
    match error {
        SectionWorkflowError::Decode { .. } => OpsCode::DecodeFailed,
        SectionWorkflowError::SectionOutOfRange { .. } => OpsCode::SectionOutOfRange,
        SectionWorkflowError::SectionIndexMismatch { .. } => OpsCode::SectionIndexMismatch,
        SectionWorkflowError::PreservingPatch(_) => OpsCode::PatchFailed,
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn stamper_code(error: &StamperError) -> OpsCode {
    match error {
        StamperError::Codec(_) => OpsCode::StampCodecFailed,
        StamperError::NotRoundTripSafe { .. } => OpsCode::InputNotRoundtripSafe,
        StamperError::UncarriedZipEntries { .. } => OpsCode::InputEntriesNotCarried,
        StamperError::Stamp(inner) => stamp_code(inner),
        StamperError::ManifestInvariant { .. } => OpsCode::StampManifestInvariant,
        StamperError::SourceHashMismatch { .. } => OpsCode::StampSourceHashMismatch,
        StamperError::CellStamp(inner) => cell_stamp_code(inner),
        StamperError::DeltaMismatch { .. } => OpsCode::StampDeltaMismatch,
        StamperError::SemanticLoss { .. } => OpsCode::EncodeSemanticLoss,
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn stamp_code(error: &StampError) -> OpsCode {
    match error {
        StampError::UnknownSpec { .. } => OpsCode::StampSpecStale,
        StampError::MarkerMismatch { .. } => OpsCode::StampMarkerMismatch,
        StampError::DuplicateSpec { .. } => OpsCode::StampSpecDuplicate,
        StampError::DuplicateName { .. } => OpsCode::StampNameDuplicate,
        StampError::NameCollision { .. } => OpsCode::StampNameCollision,
        StampError::UncoveredCandidate { .. } => OpsCode::StampCandidateUncovered,
        StampError::EmptyName => OpsCode::StampNameEmpty,
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn cell_stamp_code(error: &CellStampError) -> OpsCode {
    match error {
        CellStampError::TableNotFound { .. } => OpsCode::TableNotFound,
        CellStampError::TableGridInvalid { .. } => OpsCode::TableGridInvalid,
        CellStampError::NotAnAnchor { .. } => OpsCode::StampCellNotAnchor,
        CellStampError::TargetNotStampable { .. } => OpsCode::StampCellNotEmpty,
        CellStampError::LabelDrift { .. } => OpsCode::StampLabelDrift,
        CellStampError::UnknownCandidate { .. } => OpsCode::StampCellNotCandidate,
        CellStampError::DuplicateTarget { .. } => OpsCode::StampCellTargetDuplicate,
        CellStampError::EmptyName => OpsCode::StampNameEmpty,
        CellStampError::BlankHint { .. } => OpsCode::StampHintBlank,
        CellStampError::DuplicateName { .. } => OpsCode::StampNameDuplicate,
        CellStampError::NameCollision { .. } => OpsCode::StampNameCollision,
        CellStampError::UncoveredCandidate { .. } => OpsCode::StampCandidateUncovered,
        _ => OpsCode::UpstreamUnmapped,
    }
}

fn stamp_map_code(error: &StampMapError) -> OpsCode {
    match error {
        StampMapError::Parse(_)
        | StampMapError::UnsupportedShape
        | StampMapError::UnsupportedVersion(_)
        | StampMapError::InvalidSourceHash(_)
        | StampMapError::BlankHint { .. } => OpsCode::InvalidStampMap,
        _ => OpsCode::UpstreamUnmapped,
    }
}

// ── warnings ────────────────────────────────────────────────────

/// Stable warning code for an encode warning that this version does not know.
const UNCLASSIFIED: &str = "OTHER";

/// A non-fatal diagnostic an operation surfaced.
///
/// Peer warnings are wrapped rather than normalised; [`OpsWarning::info`]
/// produces the serialisable [`WarningInfo`] a frontend reports.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpsWarning {
    /// HWPX encoder warning.
    Encode(EncodeWarning),
    /// HWPX decoder warning.
    Decode(DecodeWarning),
    /// Markdown encoder warning.
    #[cfg(feature = "ops-md")]
    Md(MdWarning),
    /// Result of one planned asset (image) reference.
    #[cfg(feature = "ops-md")]
    Asset(AssetOutcome),
}

impl OpsWarning {
    /// The wire shape of this warning: stable code, message, optional hint.
    ///
    /// The codes reproduce the strings the CLI prints today
    /// (`LAYOUT_CACHE_DROPPED`, `UNKNOWN_ENUM_VALUE`,
    /// `TABLE_MERGE_FLATTENED`, …); `OTHER` is the same fallback the CLI
    /// uses for a warning variant it does not know.
    #[must_use]
    pub fn info(&self) -> WarningInfo {
        match self {
            Self::Encode(w) => WarningInfo::new(encode_warning_code(w), w.to_string()),
            Self::Decode(w) => WarningInfo::new(decode_warning_code(w), decode_warning_message(w)),
            #[cfg(feature = "ops-md")]
            Self::Md(w) => WarningInfo::new(md_warning_code(w), w.to_string()),
            #[cfg(feature = "ops-md")]
            Self::Asset(o) => WarningInfo::new(asset_outcome_code(o), asset_outcome_message(o)),
        }
    }
}

fn encode_warning_code(warning: &EncodeWarning) -> &'static str {
    match warning {
        EncodeWarning::LayoutCacheDropped { .. } => "LAYOUT_CACHE_DROPPED",
        EncodeWarning::NoteHeadSkipped { .. } => "NOTE_HEAD_SKIPPED",
        EncodeWarning::TitleMarkSkipped { .. } => "TITLE_MARK_SKIPPED",
        EncodeWarning::NoteRestartIgnored { .. } => "NOTE_RESTART_IGNORED",
        _ => UNCLASSIFIED,
    }
}

fn decode_warning_code(warning: &DecodeWarning) -> &'static str {
    match warning {
        DecodeWarning::UnknownEnumValue { .. } => "UNKNOWN_ENUM_VALUE",
        DecodeWarning::LayoutCacheDropped { .. } => "LAYOUT_CACHE_DROPPED",
        _ => UNCLASSIFIED,
    }
}

// `DecodeWarning` has no `Display` impl, so the message is built here — the
// same shape the CLI's `to-pdf` warning DTO prints.
fn decode_warning_message(warning: &DecodeWarning) -> String {
    match warning {
        DecodeWarning::UnknownEnumValue { attribute, raw, fallback } => {
            format!("{attribute}: \"{raw}\" unknown — fell back to {fallback}")
        }
        DecodeWarning::LayoutCacheDropped { path, reason } => {
            format!("layout cache dropped at {path}: {reason}")
        }
        other => format!("{other:?}"),
    }
}

#[cfg(feature = "ops-md")]
fn md_warning_code(warning: &MdWarning) -> &'static str {
    match warning {
        MdWarning::MergedCellsFlattened { .. } => "TABLE_MERGE_FLATTENED",
        MdWarning::ImageEmbedSkipped { .. } => "IMAGE_EMBED_SKIPPED",
        _ => UNCLASSIFIED,
    }
}

// An `Embedded` outcome is not a defect and operations do not report it as a
// warning; the arm exists so the classification is total and the inventory
// test can prove that no outcome goes unclassified.
#[cfg(feature = "ops-md")]
fn asset_outcome_code(outcome: &AssetOutcome) -> &'static str {
    match outcome {
        AssetOutcome::Embedded { .. } => "ASSET_EMBEDDED",
        AssetOutcome::Dropped { .. } => "ASSET_DROPPED",
        AssetOutcome::Remote { .. } => "ASSET_REMOTE",
        _ => UNCLASSIFIED,
    }
}

#[cfg(feature = "ops-md")]
fn asset_outcome_message(outcome: &AssetOutcome) -> String {
    match outcome {
        AssetOutcome::Embedded { occurrence, key, format } => {
            format!("{occurrence}: embedded as {key} ({format:?})")
        }
        AssetOutcome::Dropped { occurrence, reason } => format!("{occurrence}: dropped — {reason}"),
        AssetOutcome::Remote { occurrence } => {
            format!("{occurrence}: remote URL, not fetched")
        }
        other => format!("{other:?}"),
    }
}

// ── fail-closed helper ──────────────────────────────────────────

/// Takes the bytes of a regenerating edit, or refuses them when encoding
/// reported a semantic loss.
///
/// Regenerating edits (stamping, cell edits, restyle) are preserve-first: if
/// the encode lost meaning, the caller must get an error instead of bytes,
/// because the admission gate compares Core documents and cannot see damage
/// that happened on the wire. The classification lives in exactly one place,
/// [`EncodeWarning::is_semantic_loss`].
///
/// On refusal both warning lists are carried: `warnings` are the semantic
/// losses that caused it, `others` the remaining warnings of the same
/// encode, so nothing is dropped silently.
///
/// # Errors
///
/// [`OpsError::EncodeSemanticLoss`] when any warning is a semantic loss.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "ops-hwpx")] {
/// use hwpforge::hwpx::{EncodeOutcome, EncodeWarning};
/// use hwpforge::ops::take_bytes_fail_closed;
///
/// let clean = EncodeOutcome { bytes: vec![1, 2, 3], warnings: Vec::new() };
/// let (bytes, warnings) = take_bytes_fail_closed(clean).unwrap();
/// assert_eq!(bytes, vec![1, 2, 3]);
/// assert!(warnings.is_empty());
/// # }
/// ```
pub fn take_bytes_fail_closed(
    outcome: EncodeOutcome,
) -> Result<(Vec<u8>, Vec<OpsWarning>), OpsError> {
    let EncodeOutcome { bytes, warnings } = outcome;
    if warnings.iter().any(EncodeWarning::is_semantic_loss) {
        let (lost, others): (Vec<_>, Vec<_>) =
            warnings.into_iter().partition(EncodeWarning::is_semantic_loss);
        return Err(OpsError::EncodeSemanticLoss {
            warnings: lost.iter().map(|w| OpsWarning::Encode(w.clone()).info()).collect(),
            others: others.iter().map(|w| OpsWarning::Encode(w.clone()).info()).collect(),
        });
    }
    Ok((bytes, warnings.into_iter().map(OpsWarning::Encode).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_smithy_hwpx::{ParagraphPath, PathSeg};

    fn path() -> ParagraphPath {
        ParagraphPath(vec![PathSeg::Section(0)])
    }

    fn cache_dropped() -> EncodeWarning {
        EncodeWarning::LayoutCacheDropped { path: path(), reason: "ledger".into() }
    }

    fn note_head_skipped() -> EncodeWarning {
        EncodeWarning::NoteHeadSkipped { path: path(), reason: "titleMark".into() }
    }

    #[test]
    fn ops_owned_variants_carry_their_own_codes() {
        let cases: Vec<(OpsError, OpsCode)> = vec![
            (
                OpsError::Json(serde_json::from_str::<serde_json::Value>("{").unwrap_err()),
                OpsCode::JsonParseFailed,
            ),
            (OpsError::PresetNotFound { name: "gov".into() }, OpsCode::PresetNotFound),
            (OpsError::InvalidInput { reason: "two targets".into() }, OpsCode::InvalidInput),
            (OpsError::NoFonts, OpsCode::NoFonts),
            (
                OpsError::EncodeSemanticLoss { warnings: Vec::new(), others: Vec::new() },
                OpsCode::EncodeSemanticLoss,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.code(), expected, "{error}");
            assert!(!error.to_string().is_empty());
        }
    }

    #[test]
    fn every_code_has_a_hint_or_deliberately_none() {
        // Touches every arm of the hint table, and pins that a hint is never
        // an empty string (a frontend would print a blank line).
        for code in OpsCode::ALL {
            if let Some(hint) = hint_for(*code) {
                assert!(!hint.trim().is_empty(), "{code} has a blank hint");
            }
        }
        assert!(hint_for(OpsCode::UpstreamUnmapped).is_some(), "unknown errors need a hint");
        assert!(hint_for(OpsCode::JsonParseFailed).is_none(), "no CLI hint to reproduce");
    }

    #[test]
    fn clean_encode_passes_its_bytes_and_warnings_through() {
        let outcome = EncodeOutcome { bytes: vec![0x50, 0x4b], warnings: vec![cache_dropped()] };

        let (bytes, warnings) = take_bytes_fail_closed(outcome).expect("no semantic loss");

        assert_eq!(bytes, vec![0x50, 0x4b]);
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].info().code, "LAYOUT_CACHE_DROPPED");
    }

    #[test]
    fn semantic_loss_refuses_the_bytes_and_keeps_both_warning_lists() {
        let outcome = EncodeOutcome {
            bytes: vec![0x50, 0x4b],
            warnings: vec![cache_dropped(), note_head_skipped()],
        };

        let error = take_bytes_fail_closed(outcome).expect_err("must fail closed");

        assert_eq!(error.code(), OpsCode::EncodeSemanticLoss);
        let OpsError::EncodeSemanticLoss { warnings, others } = &error else {
            panic!("wrong variant: {error:?}");
        };
        assert_eq!(warnings.len(), 1, "only the semantic loss blocks the edit");
        assert_eq!(warnings[0].code, "NOTE_HEAD_SKIPPED");
        assert_eq!(others.len(), 1, "non-semantic warnings are kept, not dropped");
        assert_eq!(others[0].code, "LAYOUT_CACHE_DROPPED");
    }

    #[test]
    fn empty_encode_is_not_a_semantic_loss() {
        let (bytes, warnings) =
            take_bytes_fail_closed(EncodeOutcome { bytes: Vec::new(), warnings: Vec::new() })
                .expect("no warnings");

        assert!(bytes.is_empty());
        assert!(warnings.is_empty());
    }
}
