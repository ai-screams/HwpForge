//! Cross-frontend diagnostics: stable operation codes and the shared warning payload.
//!
//! This module is a deliberate, documented exception to Foundation's
//! "primitives only" role. The shared `ops` layer that CLI, MCP and the
//! Python bindings call needs one code table that every crate can see
//! without a reverse dependency, and the workspace does not add crates for
//! this purpose, so the table lives here.
//!
//! Isolation rules (keep them when extending):
//!
//! - nothing in this module depends on any other Foundation module;
//! - it does not extend the numeric [`crate::ErrorCode`] scheme — codes here
//!   are stable **strings** (the CLI contract), never numbers;
//! - no frontend response DTO lives here; [`WarningInfo`] is the only
//!   serialisable payload, and it is the wire shape of one warning.
//!
//! Naming follows the CLI contract: `<AREA>_<NOUN>` and `_FAILED` for
//! failures, in SCREAMING_SNAKE_CASE. Where the CLI historically used two
//! spellings for one meaning, this table holds the canonical one and the
//! frontends keep their legacy spelling through a compatibility table.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Declares [`OpsCode`] once: variant, wire string and doc line.
///
/// Generates the enum, `as_str`, `ALL` and `from_str` from a single list so
/// the three can never drift apart.
macro_rules! ops_codes {
    ($( $(#[$doc:meta])* $variant:ident => $wire:literal ),+ $(,)?) => {
        /// Stable, machine-readable code for an operation failure class.
        ///
        /// The string form ([`OpsCode::as_str`]) is the public contract that
        /// CLI JSON errors, MCP tool errors and Python `HwpForgeError.code`
        /// expose. Variants are additive; the enum is `#[non_exhaustive]`, so
        /// match on it with a wildcard arm outside this crate.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum OpsCode {
            $( $(#[$doc])* $variant, )+
        }

        impl OpsCode {
            /// Every code, in declaration order (for inventories and snapshots).
            pub const ALL: &'static [OpsCode] = &[ $( OpsCode::$variant, )+ ];

            /// The stable wire string, e.g. `"DECODE_FAILED"`.
            pub const fn as_str(self) -> &'static str {
                match self {
                    $( OpsCode::$variant => $wire, )+
                }
            }

            /// Looks a wire string up (exact match only).
            pub fn lookup(wire: &str) -> Option<OpsCode> {
                match wire {
                    $( $wire => Some(OpsCode::$variant), )+
                    _ => None,
                }
            }
        }
    };
}

ops_codes! {
    // ── generic ────────────────────────────────────────────────────
    /// Arguments were syntactically valid but semantically unusable (mutually exclusive targets, empty lists, …).
    InvalidInput => "INVALID_INPUT",
    /// An upstream error variant that this table has no mapping for yet; the message keeps the original type name.
    UpstreamUnmapped => "UPSTREAM_UNMAPPED",
    /// A library invariant was violated — a bug, not a user error.
    InternalInvariant => "INTERNAL_INVARIANT",
    /// JSON input could not be parsed.
    JsonParseFailed => "JSON_PARSE_FAILED",
    /// A result could not be serialised to JSON.
    JsonSerializeFailed => "JSON_SERIALIZE_FAILED",
    /// Requested JSON schema kind does not exist.
    UnknownSchemaType => "UNKNOWN_SCHEMA_TYPE",
    /// Named style preset does not exist.
    PresetNotFound => "PRESET_NOT_FOUND",
    // ── codec ──────────────────────────────────────────────────────
    /// HWPX package or XML could not be decoded.
    DecodeFailed => "DECODE_FAILED",
    /// HWPX (or Markdown) output could not be encoded.
    EncodeFailed => "ENCODE_FAILED",
    /// A regenerating edit refused to emit bytes because encoding reported semantic-loss warnings (fail-closed).
    EncodeSemanticLoss => "ENCODE_SEMANTIC_LOSS",
    /// Core document validation failed.
    ValidationFailed => "VALIDATION_FAILED",
    /// Markdown input could not be decoded.
    MdDecodeFailed => "MD_DECODE_FAILED",
    /// Style store could not be built from the preset.
    StyleStoreFailed => "STYLE_STORE_FAILED",
    /// Style references could not be rebound to the preset.
    StyleRebindFailed => "STYLE_REBIND_FAILED",
    /// Document analysis (inspect) failed.
    AnalysisFailed => "ANALYSIS_FAILED",
    /// A table cell grid address is invalid.
    GridAddrInvalid => "GRID_ADDR_INVALID",
    /// A table grid could not be projected for addressing.
    GridAddrProjectionFailed => "GRID_ADDR_PROJECTION_FAILED",
    // ── fill ───────────────────────────────────────────────────────
    /// A field value was empty (clearing a field is unsupported).
    EmptyFieldValue => "EMPTY_FIELD_VALUE",
    /// Named field does not exist in the document.
    FieldNotFound => "FIELD_NOT_FOUND",
    /// Several fields share the requested name.
    FieldNameAmbiguous => "FIELD_NAME_AMBIGUOUS",
    /// The field exists but is not a fillable click-here field.
    FieldNotFillable => "FIELD_NOT_FILLABLE",
    /// Fill workflow failed for another reason.
    FillFailed => "FILL_FAILED",
    /// No field values were supplied.
    NoValues => "NO_VALUES",
    // ── section workflow ───────────────────────────────────────────
    /// Requested section index is outside the document.
    SectionOutOfRange => "SECTION_OUT_OF_RANGE",
    /// Section index in the payload does not match the requested one.
    SectionIndexMismatch => "SECTION_INDEX_MISMATCH",
    /// Preserving patch could not be applied.
    PatchFailed => "PATCH_FAILED",
    /// Section export/patch workflow failed for another reason.
    SectionWorkflowFailed => "SECTION_WORKFLOW_FAILED",
    // ── read ───────────────────────────────────────────────────────
    /// `read` needs exactly one target (section, paragraphs, table or field).
    ReadTargetRequired => "READ_TARGET_REQUIRED",
    /// Paragraph range expression could not be parsed.
    ReadParasInvalid => "READ_PARAS_INVALID",
    /// Paragraph range given without a section.
    ReadParasWithoutSection => "READ_PARAS_WITHOUT_SECTION",
    /// `read` section index is outside the document.
    ReadSectionOutOfRange => "READ_SECTION_OUT_OF_RANGE",
    /// `read` paragraph range is outside the section.
    ReadParaRangeInvalid => "READ_PARA_RANGE_INVALID",
    /// `read` table ordinal is outside the document.
    ReadTableOutOfRange => "READ_TABLE_OUT_OF_RANGE",
    /// `read` field name does not exist.
    ReadFieldNotFound => "READ_FIELD_NOT_FOUND",
    // ── table / cell ───────────────────────────────────────────────
    /// Table ordinal does not exist.
    TableNotFound => "TABLE_NOT_FOUND",
    /// Table grid is malformed.
    TableGridInvalid => "TABLE_GRID_INVALID",
    /// Table grid cannot be addressed by label.
    TableGridUnaddressable => "TABLE_GRID_UNADDRESSABLE",
    /// Addressed cell does not exist.
    CellNotFound => "CELL_NOT_FOUND",
    /// Cell label matches more than one cell.
    CellLabelAmbiguous => "CELL_LABEL_AMBIGUOUS",
    /// Target cell contains non-text content that an edit would destroy.
    CellHasNonTextContent => "CELL_HAS_NON_TEXT_CONTENT",
    /// The same cell was targeted twice.
    CellTargetDuplicate => "CELL_TARGET_DUPLICATE",
    /// Two cell specs resolve to conflicting targets.
    CellTargetConflict => "CELL_TARGET_CONFLICT",
    /// Input document is not round-trip safe, so a regenerating edit was refused.
    InputNotRoundtripSafe => "INPUT_NOT_ROUNDTRIP_SAFE",
    /// ZIP entries of the input would be lost by re-encoding, so the edit was refused.
    InputEntriesNotCarried => "INPUT_ENTRIES_NOT_CARRIED",
    /// Cell edit failed inside the codec.
    SetCellCodecFailed => "SET_CELL_CODEC_FAILED",
    /// Cell edit failed for another reason.
    SetCellFailed => "SET_CELL_FAILED",
    /// Cell edit arguments are inconsistent (single target and spec list mixed, …).
    InvalidSetCellArgs => "INVALID_SET_CELL_ARGS",
    /// Cell edit spec list is malformed.
    InvalidSetCellMap => "INVALID_SET_CELL_MAP",
    // ── stamp ──────────────────────────────────────────────────────
    /// Stamping failed for another reason.
    StampFailed => "STAMP_FAILED",
    /// Stamping failed inside the codec.
    StampCodecFailed => "STAMP_CODEC_FAILED",
    /// Stamp manifest violated an invariant.
    StampManifestInvariant => "STAMP_MANIFEST_INVARIANT",
    /// Stamp request was made for a different source document (hash mismatch).
    StampSourceHashMismatch => "STAMP_SOURCE_HASH_MISMATCH",
    /// Stamp output failed self-verification.
    StampDeltaMismatch => "STAMP_DELTA_MISMATCH",
    /// Cell stamp target is not the anchor of its merged region.
    StampCellNotAnchor => "STAMP_CELL_NOT_ANCHOR",
    /// Cell stamp target is not an empty cell.
    StampCellNotEmpty => "STAMP_CELL_NOT_EMPTY",
    /// Cell label drifted since the plan was made.
    StampLabelDrift => "STAMP_LABEL_DRIFT",
    /// Cell stamp target is not a plan candidate.
    StampCellNotCandidate => "STAMP_CELL_NOT_CANDIDATE",
    /// The same cell was stamped twice.
    StampCellTargetDuplicate => "STAMP_CELL_TARGET_DUPLICATE",
    /// Stamp field name is empty.
    StampNameEmpty => "STAMP_NAME_EMPTY",
    /// Cell stamp hint is blank.
    StampHintBlank => "STAMP_HINT_BLANK",
    /// Stamp field name is used twice in the request.
    StampNameDuplicate => "STAMP_NAME_DUPLICATE",
    /// Stamp field name collides with an existing field.
    StampNameCollision => "STAMP_NAME_COLLISION",
    /// A plan candidate was neither named nor ignored.
    StampCandidateUncovered => "STAMP_CANDIDATE_UNCOVERED",
    /// Stamp spec no longer matches the document.
    StampSpecStale => "STAMP_SPEC_STALE",
    /// Stamp marker text differs from the plan.
    StampMarkerMismatch => "STAMP_MARKER_MISMATCH",
    /// The same span was specified twice.
    StampSpecDuplicate => "STAMP_SPEC_DUPLICATE",
    /// Stamp request payload is malformed.
    InvalidStampMap => "INVALID_STAMP_MAP",
    /// A v2 stamp request lacks `source_sha256`.
    MissingSourceSha256 => "MISSING_SOURCE_SHA256",
    // ── structural edit ────────────────────────────────────────────
    /// Structural edit failed for another reason.
    StructuralEditFailed => "STRUCTURAL_EDIT_FAILED",
    /// Structural edit failed inside the codec.
    StructuralCodec => "STRUCTURAL_CODEC",
    /// Paragraph index is outside the section.
    ParagraphOutOfRange => "PARAGRAPH_OUT_OF_RANGE",
    /// The same paragraph was targeted twice.
    DuplicateTarget => "DUPLICATE_TARGET",
    /// Deleting would strand a reference (note, bookmark, …).
    ReferenceStranded => "REFERENCE_STRANDED",
    /// Edit would lose a hard break.
    HardBreakLoss => "HARD_BREAK_LOSS",
    /// Edit would leave a section empty.
    EmptySection => "EMPTY_SECTION",
    /// The section-properties paragraph cannot be edited.
    SectionPropertiesParagraph => "SECTION_PROPERTIES_PARAGRAPH",
    /// Span count changed unexpectedly.
    SpanCountMismatch => "SPAN_COUNT_MISMATCH",
    /// Structural edit output failed self-verification.
    SelfVerifyFailed => "SELF_VERIFY_FAILED",
    /// Inserted text spans several paragraphs where one is required.
    MultiParagraphText => "MULTI_PARAGRAPH_TEXT",
    /// Insertion before the section-properties paragraph is not allowed.
    InsertBeforeSectionProperties => "INSERT_BEFORE_SECTION_PROPERTIES",
    /// `delete_para` was given no target.
    DeleteNoTarget => "DELETE_NO_TARGET",
    /// `insert_para` was given no text.
    InsertTextRequired => "INSERT_TEXT_REQUIRED",
    // ── conversion (HWP5 · PDF) ────────────────────────────────────
    /// HWP5 input could not be decoded.
    Hwp5DecodeFailed => "HWP5_DECODE_FAILED",
    /// HWP5 → HWPX conversion failed.
    Hwp5ConvertFailed => "HWP5_CONVERT_FAILED",
    /// Input bytes are neither HWP5 nor HWPX.
    UnrecognizedFormat => "UNRECOGNIZED_FORMAT",
    /// PDF rendering failed (the renderer's own code is carried as the cause).
    PdfRenderFailed => "PDF_RENDER_FAILED",
    /// Font discovery mode is not one of the accepted values.
    InvalidDiscovery => "INVALID_DISCOVERY",
}

impl fmt::Display for OpsCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A wire string that is not an [`OpsCode`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownOpsCode(pub String);

impl fmt::Display for UnknownOpsCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown ops code `{}`", self.0)
    }
}

impl std::error::Error for UnknownOpsCode {}

impl std::str::FromStr for OpsCode {
    type Err = UnknownOpsCode;

    fn from_str(wire: &str) -> Result<Self, Self::Err> {
        OpsCode::lookup(wire).ok_or_else(|| UnknownOpsCode(wire.to_owned()))
    }
}

impl Serialize for OpsCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for OpsCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = String::deserialize(deserializer)?;
        wire.parse().map_err(serde::de::Error::custom)
    }
}

/// One non-fatal warning as every frontend reports it.
///
/// `code` is a stable SCREAMING_SNAKE string (an [`OpsCode`] wire string or a
/// crate-specific warning code such as `LAYOUT_CACHE_DROPPED`), `message` is
/// human-readable, and `hint` is an optional recovery suggestion. The struct
/// is `#[non_exhaustive]`; build it with [`WarningInfo::new`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[non_exhaustive]
pub struct WarningInfo {
    /// Stable warning code.
    pub code: String,
    /// Human-readable description.
    pub message: String,
    /// Optional recovery hint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl WarningInfo {
    /// Creates a warning with `code` and `message` and no hint.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into(), hint: None }
    }

    /// Creates a warning whose code is an [`OpsCode`].
    pub fn coded(code: OpsCode, message: impl Into<String>) -> Self {
        Self::new(code.as_str(), message)
    }

    /// Attaches a recovery hint.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_code_round_trips_through_its_wire_string() {
        for code in OpsCode::ALL {
            assert_eq!(code.as_str().parse::<OpsCode>(), Ok(*code), "{code:?}");
            assert_eq!(OpsCode::lookup(code.as_str()), Some(*code));
        }
    }

    #[test]
    fn wire_strings_are_unique_screaming_snake() {
        let mut seen = HashSet::new();
        for code in OpsCode::ALL {
            let s = code.as_str();
            assert!(seen.insert(s), "duplicate wire string {s}");
            assert!(
                s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'),
                "{s} is not SCREAMING_SNAKE"
            );
            assert!(!s.starts_with('_') && !s.ends_with('_'), "{s}");
        }
    }

    #[test]
    fn serde_uses_the_wire_string() {
        let json = serde_json::to_string(&OpsCode::EncodeSemanticLoss).unwrap();
        assert_eq!(json, "\"ENCODE_SEMANTIC_LOSS\"");
        let back: OpsCode = serde_json::from_str(&json).unwrap();
        assert_eq!(back, OpsCode::EncodeSemanticLoss);
        assert!(serde_json::from_str::<OpsCode>("\"NOPE\"").is_err());
    }

    #[test]
    fn unknown_wire_string_is_none() {
        assert_eq!(OpsCode::lookup("decode_failed"), None);
        assert_eq!("".parse::<OpsCode>(), Err(UnknownOpsCode(String::new())));
        assert_eq!(UnknownOpsCode("X".into()).to_string(), "unknown ops code `X`");
    }

    #[test]
    fn warning_info_serialises_without_null_hint() {
        let w = WarningInfo::coded(OpsCode::EncodeSemanticLoss, "note head skipped");
        assert_eq!(
            serde_json::to_string(&w).unwrap(),
            r#"{"code":"ENCODE_SEMANTIC_LOSS","message":"note head skipped"}"#
        );
        let w = w.with_hint("re-export the section");
        let v: serde_json::Value = serde_json::to_value(&w).unwrap();
        assert_eq!(v["hint"], "re-export the section");
        let back: WarningInfo = serde_json::from_value(v).unwrap();
        assert_eq!(back, w);
    }

    #[test]
    fn display_is_the_wire_string() {
        assert_eq!(OpsCode::PresetNotFound.to_string(), "PRESET_NOT_FOUND");
    }
}
