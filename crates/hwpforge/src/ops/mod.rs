//! Shared operation layer — one implementation of each document operation,
//! called by every frontend (CLI, MCP server, Python bindings).
//!
//! # What lives here
//!
//! One free function per operation. The input is bytes (`&[u8]` for HWPX) or
//! text (`&str` for Markdown and JSON), options are a [`Default`] struct with
//! consuming `with_*` builders, and the output is
//! `Result<XxxOutput, OpsError>`. **Operation functions never touch the
//! filesystem**; the one place that does is the `fs` submodule, and it does
//! so for exactly two reasons: reading a whole input document from a path
//! under a caller-chosen size cap ([`fs::read_bounded`], feature `ops-hwpx`
//! — W6b audit follow-up), the one frontend-shared input size gate CLI, MCP
//! and the Python bindings all read through; and resolving the `file:`
//! entries of an asset plan a caller already made
//! ([`fs::resolve_files_from_dir`], feature `ops-md` — it needs the Markdown
//! smithy's asset plan type).
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
//! Codec errors are **stage-tagged**: the same [`HwpxError`] variant means
//! `DECODE_FAILED` when reading a package and `ENCODE_FAILED` when writing
//! one, exactly as the CLI reports it today, so there is no `From<HwpxError>`
//! — an operation says which stage failed through [`OpsError::decode`] /
//! [`OpsError::encode`] (and `md_decode` / `md_encode` for Markdown). A
//! nested Core or Foundation failure inside a codec error takes the stage's
//! code too; only a direct [`OpsError::Core`] (document validation) is
//! `VALIDATION_FAILED`.
//!
//! # Features
//!
//! `ops-hwpx` gives the HWPX-only operations, `ops-md` adds the Markdown
//! ones, and `ops` is the alias for everything.

#[cfg(feature = "ops-md")]
pub mod convert;
pub mod diff;
pub mod edit;
pub mod exchange;
#[cfg(feature = "ops-hwpx")]
pub mod fs;
pub mod inspect;
pub mod inspect_meta;
#[cfg(feature = "ops-md")]
pub mod markdown;
pub mod read;
#[cfg(feature = "schemars")]
pub mod schema;
pub mod stamp;
pub mod style;
mod walk;

use hwpforge_core::CoreError;
use hwpforge_foundation::diagnostics::{OpsCode, WarningInfo};
use hwpforge_foundation::FoundationError;
use hwpforge_smithy_hwpx::{
    grid_addr::{GridAddrError, GridAddrWarning},
    partition_semantic_loss,
    stamp::{CellStampError, StampError, StampMapError, StamperError},
    CellEditError, DecodeWarning, EncodeOutcome, EncodeWarning, FillError, HwpxError,
    HwpxErrorCode, ReadError, SectionWorkflowError, SectionWorkflowWarning, StructuralEditError,
    StructuralWarning,
};

#[cfg(feature = "ops-md")]
use hwpforge_smithy_md::{assets::AssetOutcome, MdError, MdErrorCode, MdWarning};

pub use inspect::{inspect, InspectOptions, InspectOutput, InspectReport};
pub use inspect_meta::InspectMeta;

#[cfg(feature = "ops-md")]
pub use convert::{convert_md, decode_md, ConvertMdOptions, ConvertMeta, ConvertOutput, MdDecoded};
pub use diff::{diff, DiffMeta, DiffOutput};
pub use edit::{
    delete_para, fill, insert_para, set_cell, CellSpec, DeleteParaOptions, FillMeta, FillOptions,
    FillOutput, InsertParaOptions, SetCellMeta, SetCellOptions, SetCellOutput, StructuralMeta,
    StructuralOutput,
};
pub use exchange::{
    export_section, from_json, patch, to_json, EncodeMeta, ExportSectionMeta, ExportSectionOptions,
    ExportSectionOutput, FromJsonOptions, FromJsonOutput, PatchMeta, PatchOptions, PatchOutput,
    ToJsonMeta, ToJsonOptions, ToJsonOutput,
};
#[cfg(feature = "ops-md")]
pub use markdown::{to_md, MdExportMeta, MdExportOptions, MdExportOutput, MdMode};
pub use read::{
    fields, outline, read, FieldsMeta, FieldsOutput, OutlineMeta, OutlineOutput, ReadMeta,
    ReadOptions, ReadOutput,
};
#[cfg(feature = "schemars")]
pub use schema::{schema, SchemaKind, SchemaOptions, SchemaOutput};
pub use stamp::{
    default_manifest_path, stamp, stamp_plan, CellStampSpec, StampMeta, StampOptions, StampOutput,
    StampPlanMeta, StampPlanOutput, StampSpec, StampedManifest,
};
pub use style::{
    restyle, templates, validate, RestyleMeta, RestyleOptions, RestyleOutput, TemplateList,
    TemplatesOutput, ValidateOutput, ValidateReport,
};

// ── errors ──────────────────────────────────────────────────────

/// Everything an operation in this module can fail with.
///
/// Library errors are wrapped, not normalised: the original error keeps its
/// own message and source chain, and [`OpsError::code`] adds the stable
/// classification that frontends report.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OpsError {
    /// HWPX decode-stage failure: reading the package, parsing XML or
    /// projecting it to Core. Build it with [`OpsError::decode`].
    #[error(transparent)]
    Decode(HwpxError),

    /// HWPX encode-stage failure: serialising XML or writing the package.
    /// Build it with [`OpsError::encode`].
    #[error(transparent)]
    Encode(HwpxError),

    /// Markdown decode-stage failure (parsing Markdown, resolving assets).
    /// Build it with [`OpsError::md_decode`].
    #[cfg(feature = "ops-md")]
    #[error(transparent)]
    MdDecode(MdError),

    /// Markdown encode-stage failure (exporting to Markdown).
    /// Build it with [`OpsError::md_encode`].
    #[cfg(feature = "ops-md")]
    #[error(transparent)]
    MdEncode(MdError),

    /// Foundation primitive invariant failure that reached an operation
    /// directly (bad unit, colour, index or identifier).
    #[error(transparent)]
    Foundation(#[from] FoundationError),

    /// The style store could not be built from the resolved template
    /// (`STYLE_STORE_FAILED`, as the CLI reports it). Build it with
    /// [`OpsError::style_store`].
    #[error("style store error: {0}")]
    StyleStore(HwpxError),

    /// Style references could not be rebound onto the store
    /// (`STYLE_REBIND_FAILED`). Build it with [`OpsError::style_rebind`].
    #[error("style rebind error: {0}")]
    StyleRebind(HwpxError),

    /// Click-here field fill failure.
    #[error(transparent)]
    Fill(#[from] FillError),

    /// Table cell edit failure. A `CellEditError::SemanticLoss` never
    /// arrives here: `From<CellEditError>` turns it into
    /// [`OpsError::EncodeSemanticLoss`] so every regenerating edit reports
    /// the same envelope.
    #[error(transparent)]
    CellEdit(CellEditError),

    /// Read/projection failure (`outline`, `read`, `fields`).
    #[error(transparent)]
    Read(#[from] ReadError),

    /// Paragraph insert/delete failure.
    #[error(transparent)]
    StructuralEdit(#[from] StructuralEditError),

    /// Cell grid address **verification** failure on a JSON tree a caller
    /// supplied (`from_json`, `patch`) — `GRID_ADDR_INVALID`.
    #[error(transparent)]
    GridAddr(#[from] GridAddrError),

    /// Cell grid address **projection** failure while annotating an export
    /// (`to_json`, `export_section`) — `GRID_ADDR_PROJECTION_FAILED`. Build
    /// it with [`OpsError::grid_addr_projection`].
    #[error(transparent)]
    GridAddrProjection(GridAddrError),

    /// Section export/patch workflow failure.
    #[error(transparent)]
    SectionWorkflow(#[from] SectionWorkflowError),

    /// Template stamping failure. A `StamperError::SemanticLoss` never
    /// arrives here (see [`OpsError::CellEdit`]).
    #[error(transparent)]
    Stamper(StamperError),

    /// Stamp request (map) parse or validation failure.
    #[error(transparent)]
    StampMap(#[from] StampMapError),

    /// Core document failure, including validation.
    #[error(transparent)]
    Core(#[from] CoreError),

    /// JSON input could not be parsed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// A result could not be serialised to JSON (for example non-finite
    /// chart data). Build it with [`OpsError::json_serialize`].
    #[error("JSON serialize error: {0}")]
    JsonSerialize(serde_json::Error),

    /// The requested style preset does not exist.
    #[error("preset not found: {name}")]
    PresetNotFound {
        /// The preset name that was requested.
        name: String,
    },

    /// Arguments were syntactically valid but semantically unusable, with
    /// no more specific code than `INVALID_INPUT`.
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// What made the arguments unusable.
        reason: String,
    },

    /// An argument rejection that reports its own stable code.
    ///
    /// The frontends validate some arguments before the library sees them
    /// (an empty value map is `NO_VALUES`, mixed cell targets are
    /// `INVALID_SET_CELL_ARGS`, …). Those rejections keep their dedicated
    /// code and the frontend's wording.
    #[error("invalid input: {reason}")]
    Rejected {
        /// The stable code this rejection reports.
        code: OpsCode,
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

    /// A read through [`fs::read_bounded`] failed to open or read `path` — the
    /// wrapped [`std::io::Error`] keeps its own [`std::io::ErrorKind`]
    /// verbatim so a frontend can still special-case a missing file the way
    /// it already did before the read moved here (the size violation
    /// `read_bounded` also guards against is not an I/O failure and reports
    /// [`OpsCode::InputTooLarge`] through [`OpsError::Rejected`] instead).
    #[cfg(feature = "ops-hwpx")]
    #[error(transparent)]
    Io(std::io::Error),
}

impl OpsError {
    /// The stable code a frontend reports for this failure.
    ///
    /// See the module docs for how exhaustive and `#[non_exhaustive]`
    /// upstream enums are treated differently.
    #[must_use]
    pub fn code(&self) -> OpsCode {
        match self {
            Self::Decode(e) => hwpx_code(e, Stage::Decode),
            Self::Encode(e) => hwpx_code(e, Stage::Encode),
            // `md_code` documents why the two asset-contract variants keep
            // dedicated codes instead of the stage code.
            #[cfg(feature = "ops-md")]
            Self::MdDecode(e) => md_code(e, Stage::Decode),
            #[cfg(feature = "ops-md")]
            Self::MdEncode(e) => md_code(e, Stage::Encode),
            Self::Foundation(_) => OpsCode::InternalInvariant,
            Self::StyleStore(_) => OpsCode::StyleStoreFailed,
            Self::StyleRebind(_) => OpsCode::StyleRebindFailed,
            Self::Fill(e) => fill_code(e),
            Self::CellEdit(e) => cell_edit_code(e),
            Self::Read(e) => read_code(e),
            Self::StructuralEdit(e) => structural_code(e),
            Self::GridAddr(e) => grid_addr_code(e),
            Self::GridAddrProjection(_) => OpsCode::GridAddrProjectionFailed,
            Self::SectionWorkflow(e) => section_workflow_code(e),
            Self::Stamper(e) => stamper_code(e),
            Self::StampMap(e) => stamp_map_code(e),
            Self::Core(e) => core_code(e),
            Self::Json(_) => OpsCode::JsonParseFailed,
            Self::JsonSerialize(_) => OpsCode::JsonSerializeFailed,
            Self::PresetNotFound { .. } => OpsCode::PresetNotFound,
            Self::InvalidInput { .. } => OpsCode::InvalidInput,
            Self::Rejected { code, .. } => *code,
            Self::NoFonts => OpsCode::NoFonts,
            Self::EncodeSemanticLoss { .. } => OpsCode::EncodeSemanticLoss,
            // No frontend consults this code today — each keeps its own
            // legacy string for "the file could not be read" and only
            // special-cases the wrapped `io::ErrorKind` (see the variant
            // doc), never `OpsError::code()`. `UpstreamUnmapped` is still the
            // honest answer: `std::io::Error` has no `OpsCode` of its own.
            #[cfg(feature = "ops-hwpx")]
            Self::Io(_) => OpsCode::UpstreamUnmapped,
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

    /// Wraps an HWPX codec failure that happened while **decoding**.
    #[must_use]
    pub fn decode(error: HwpxError) -> Self {
        Self::Decode(error)
    }

    /// Wraps an HWPX codec failure that happened while **encoding**.
    #[must_use]
    pub fn encode(error: HwpxError) -> Self {
        Self::Encode(error)
    }

    /// Wraps a grid-address projection failure met while annotating an
    /// export (the CLI's `GRID_ADDR_PROJECTION_FAILED`).
    #[must_use]
    pub fn grid_addr_projection(error: GridAddrError) -> Self {
        Self::GridAddrProjection(error)
    }

    /// Wraps a JSON serialisation failure (the CLI's `JSON_SERIALIZE_FAILED`).
    #[must_use]
    pub fn json_serialize(error: serde_json::Error) -> Self {
        Self::JsonSerialize(error)
    }

    /// Wraps a failure of building the style store from a template (the
    /// CLI's `STYLE_STORE_FAILED` stage between decoding and encoding).
    #[must_use]
    pub fn style_store(error: HwpxError) -> Self {
        Self::StyleStore(error)
    }

    /// Wraps a failure of rebinding style references onto the store (the
    /// CLI's `STYLE_REBIND_FAILED` stage).
    #[must_use]
    pub fn style_rebind(error: HwpxError) -> Self {
        Self::StyleRebind(error)
    }

    /// Wraps a Markdown failure that happened while **decoding** (parsing or
    /// asset resolution).
    #[cfg(feature = "ops-md")]
    #[must_use]
    pub fn md_decode(error: MdError) -> Self {
        Self::MdDecode(error)
    }

    /// Wraps a Markdown failure that happened while **encoding** (export).
    #[cfg(feature = "ops-md")]
    #[must_use]
    pub fn md_encode(error: MdError) -> Self {
        Self::MdEncode(error)
    }

    /// The uniform fail-closed envelope: both lists keep their original
    /// order and become [`WarningInfo`] through [`OpsWarning::info`].
    #[must_use]
    pub fn semantic_loss(warnings: Vec<EncodeWarning>, others: Vec<EncodeWarning>) -> Self {
        let info = |w: EncodeWarning| OpsWarning::Encode(w).info();
        Self::EncodeSemanticLoss {
            warnings: warnings.into_iter().map(info).collect(),
            others: others.into_iter().map(info).collect(),
        }
    }
}

impl From<CellEditError> for OpsError {
    fn from(error: CellEditError) -> Self {
        match error {
            CellEditError::SemanticLoss { warnings, others } => {
                Self::semantic_loss(warnings, others)
            }
            other => Self::CellEdit(other),
        }
    }
}

impl From<StamperError> for OpsError {
    fn from(error: StamperError) -> Self {
        match error {
            StamperError::SemanticLoss { warnings, others } => {
                Self::semantic_loss(warnings, others)
            }
            other => Self::Stamper(other),
        }
    }
}

/// Which codec stage an error came from — the stage decides the code.
#[derive(Debug, Clone, Copy)]
enum Stage {
    Decode,
    Encode,
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

// The stage decides the code: the CLI prints `DECODE_FAILED` for anything the
// decoder returns and `ENCODE_FAILED` for anything the encoder returns,
// including a nested Core or Foundation failure. Every variant this version
// knows is still listed so a new upstream variant lands on
// `UPSTREAM_UNMAPPED` and the inventory test asks for a decision.
fn hwpx_code(error: &HwpxError, stage: Stage) -> OpsCode {
    let stage_code = match stage {
        Stage::Decode => OpsCode::DecodeFailed,
        Stage::Encode => OpsCode::EncodeFailed,
    };
    match error.code() {
        HwpxErrorCode::Zip
        | HwpxErrorCode::InvalidMimetype
        | HwpxErrorCode::MissingFile
        | HwpxErrorCode::XmlParse
        | HwpxErrorCode::InvalidAttribute
        | HwpxErrorCode::IndexOutOfBounds
        | HwpxErrorCode::InvalidStructure
        | HwpxErrorCode::LayoutCacheDropped
        | HwpxErrorCode::XmlSerialize
        | HwpxErrorCode::Io
        | HwpxErrorCode::Core
        | HwpxErrorCode::Foundation => stage_code,
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

/// Classifies a Markdown codec failure by stage.
///
/// Decoding reports `MD_DECODE_FAILED` for the whole decode family — the CLI
/// wraps the entire decode in one catch, so a nested Core or Foundation
/// failure takes that code too. Encoding (Markdown export) reports
/// `ENCODE_FAILED`, as the CLI does. An oversized input keeps
/// `INPUT_TOO_LARGE` in both stages.
///
/// The two asset-contract variants get dedicated codes in both stages.
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
fn md_code(error: &MdError, stage: Stage) -> OpsCode {
    let stage_code = match stage {
        Stage::Decode => OpsCode::MdDecodeFailed,
        Stage::Encode => OpsCode::EncodeFailed,
    };
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
        | MdErrorCode::Blueprint
        | MdErrorCode::Io
        | MdErrorCode::Core
        | MdErrorCode::Foundation => stage_code,
        MdErrorCode::FileTooLarge => OpsCode::InputTooLarge,
        MdErrorCode::AssetPlanMismatch => OpsCode::AssetPlanMismatch,
        MdErrorCode::AssetIdentityConflict => OpsCode::AssetIdentityConflict,
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
    /// Advisory diagnostic from a structural paragraph edit (for example an
    /// index mark removed together with its paragraph).
    Structural(StructuralWarning),
    /// A table exported without grid addresses (`TABLE_GRID_UNADDRESSABLE`).
    GridAddr(GridAddrWarning),
    /// Section export/patch advisory (preservation metadata unavailable).
    SectionWorkflow(SectionWorkflowWarning),
    /// Markdown encoder warning.
    #[cfg(feature = "ops-md")]
    Md(MdWarning),
    /// Result of one planned asset (image) reference.
    #[cfg(feature = "ops-md")]
    Asset(AssetOutcome),
}

impl OpsWarning {
    /// Wraps an asset outcome that is worth reporting, or `None` for an
    /// `Embedded` outcome — a successful embed is not a warning and belongs
    /// only in the conversion report's `assets` list.
    #[cfg(feature = "ops-md")]
    #[must_use]
    pub fn asset(outcome: AssetOutcome) -> Option<Self> {
        match outcome {
            AssetOutcome::Embedded { .. } => None,
            other => Some(Self::Asset(other)),
        }
    }

    /// The wire shape of this warning: stable code, message, optional hint.
    ///
    /// Codes the CLI already prints today keep their string:
    /// `LAYOUT_CACHE_DROPPED`, `UNKNOWN_ENUM_VALUE`, `TABLE_MERGE_FLATTENED`,
    /// and `OTHER` for a variant this version does not know. The rest are
    /// **new canonical codes** that the frontends will adopt through their
    /// compatibility tables: `NOTE_HEAD_SKIPPED`, `TITLE_MARK_SKIPPED`,
    /// `NOTE_RESTART_IGNORED` (the CLI prints these warnings uncoded today),
    /// `IMAGE_EMBED_SKIPPED`. `ASSET_DROPPED` and `ASSET_REMOTE` are
    /// **reserved, not emitted**: `convert_md` reports an excluded image once,
    /// through the Markdown warning (`IMAGE_EMBED_SKIPPED`, the CLI's wording),
    /// and keeps the typed disposition in its `assets` list; the two codes
    /// exist for a future asset provider that reports outcomes directly. The CLI's
    /// `to-pdf` also files a decode-side `LayoutCacheDropped` under `OTHER`
    /// and carries the `UnknownEnumValue` attribute in a separate `location`
    /// field; here the attribute is part of the message.
    #[must_use]
    pub fn info(&self) -> WarningInfo {
        match self {
            Self::Encode(w) => WarningInfo::new(encode_warning_code(w), w.to_string()),
            Self::Decode(w) => WarningInfo::new(decode_warning_code(w), decode_warning_message(w)),
            Self::Structural(w) => WarningInfo::new(structural_warning_code(w), w.to_string()),
            Self::GridAddr(w) => WarningInfo::new(
                "TABLE_GRID_UNADDRESSABLE",
                format!(
                    "table #{} in section {} exported without grid addresses: {}",
                    w.table_ordinal, w.section, w.reason
                ),
            ),
            Self::SectionWorkflow(w) => WarningInfo::new(w.code(), w.message()),
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

fn structural_warning_code(warning: &StructuralWarning) -> &'static str {
    match warning {
        StructuralWarning::IndexMarkRemoved { .. } => "INDEX_MARK_REMOVED",
        _ => UNCLASSIFIED,
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
        // The split (and its order contract) lives in smithy-hwpx.
        let (lost, others) = partition_semantic_loss(warnings);
        return Err(OpsError::semantic_loss(lost, others));
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
                OpsError::Rejected { code: OpsCode::NoValues, reason: "no field values".into() },
                OpsCode::NoValues,
            ),
            (
                OpsError::json_serialize(
                    serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
                ),
                OpsCode::JsonSerializeFailed,
            ),
            (
                OpsError::style_store(HwpxError::InvalidStructure { detail: "no fonts".into() }),
                OpsCode::StyleStoreFailed,
            ),
            (
                OpsError::style_rebind(HwpxError::InvalidStructure { detail: "dangling".into() }),
                OpsCode::StyleRebindFailed,
            ),
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
