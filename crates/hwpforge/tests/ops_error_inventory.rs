//! Audit: no upstream error or warning variant goes unclassified.
//!
//! Three checks, plus the runtime cross-check:
//!
//! 1. the regenerated inventory equals the tracked JSON file;
//! 2. every wrapped enum's variants appear in the literal mapping tables
//!    below, and the tables name nothing the inventory does not have;
//! 3. no public `*Error`/`*Warning` enum in a manifest file is unnamed;
//! 4. constructing one value per variant produces the code the table claims.
//!
//! The tables are literal data on purpose: deriving them by calling
//! `code()` would make the audit agree with whatever the mapping happens to
//! do, and would follow the `ops-md` feature gate while the syn scan records
//! every syntactic variant regardless of `cfg`.
//!
//! A few variants carry another error and delegate the classification
//! (`HwpxError::Core`, `StamperError::Stamp`, …). For those the table
//! records the code of the *sample this test builds*, and
//! `core_error_gets_the_same_code_through_either_wrapper` pins the
//! delegation itself.
//!
//! Regenerate after an intentional upstream change:
//! `UPDATE_INVENTORY=1 cargo nextest run -p hwpforge --all-features`.
#![cfg(feature = "ops-md")]

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use hwpforge::core::error::{CoreError, ValidationError};
use hwpforge::core::image::ImageFormat;
use hwpforge::core::table::grid::GridCoord;
use hwpforge::foundation::diagnostics::OpsCode;
use hwpforge::foundation::FoundationError;
use hwpforge::hwpx::grid_addr::GridAddrError;
use hwpforge::hwpx::stamp::{CellStampError, StampError, StampMapError, StamperError};
use hwpforge::hwpx::{
    CellEditError, DecodeWarning, EncodeWarning, FillError, HwpxError, ParagraphPath, PathSeg,
    ReadError, SectionWorkflowError, SectionWorkflowWarning, StructuralEditError,
    StructuralWarning,
};
use hwpforge::md::assets::{AssetOutcome, RunLocator};
use hwpforge::md::embed::ImageEmbedSkipReason;
use hwpforge::md::{MdError, MdWarning};
use hwpforge::ops::{OpsError, OpsWarning};

#[path = "support/error_inventory.rs"]
mod error_inventory;

const TRACKED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/ops_error_inventory.json");

// ── the mapping tables (literal data — see the module docs) ──────

type Table = &'static [(&'static str, &'static [(&'static str, &'static str)])];

const ERROR_MAPPING: Table = &[
    (
        "HwpxError",
        &[
            ("Zip", "DECODE_FAILED"),
            ("InvalidMimetype", "DECODE_FAILED"),
            ("MissingFile", "DECODE_FAILED"),
            ("XmlParse", "DECODE_FAILED"),
            ("InvalidAttribute", "DECODE_FAILED"),
            ("IndexOutOfBounds", "DECODE_FAILED"),
            ("InvalidStructure", "DECODE_FAILED"),
            ("LayoutCacheDropped", "DECODE_FAILED"),
            ("Io", "DECODE_FAILED"),
            ("Core", "DECODE_FAILED"),
            ("Foundation", "DECODE_FAILED"),
            ("XmlSerialize", "DECODE_FAILED"),
        ],
    ),
    (
        "FoundationError",
        &[
            ("InvalidHwpUnit", "INTERNAL_INVARIANT"),
            ("InvalidColor", "INTERNAL_INVARIANT"),
            ("IndexOutOfBounds", "INTERNAL_INVARIANT"),
            ("EmptyIdentifier", "INTERNAL_INVARIANT"),
            ("InvalidField", "INTERNAL_INVARIANT"),
            ("ParseError", "INTERNAL_INVARIANT"),
        ],
    ),
    (
        "CoreError",
        &[
            ("Validation", "VALIDATION_FAILED"),
            ("Foundation", "INTERNAL_INVARIANT"),
            ("InvalidStructure", "VALIDATION_FAILED"),
        ],
    ),
    (
        "FillError",
        &[
            ("EmptyValue", "EMPTY_FIELD_VALUE"),
            ("UnknownField", "FIELD_NOT_FOUND"),
            ("DuplicateFieldName", "FIELD_NAME_AMBIGUOUS"),
            ("UnfillableField", "FIELD_NOT_FILLABLE"),
            ("Workflow", "FILL_FAILED"),
        ],
    ),
    (
        "CellEditError",
        &[
            ("TableNotFound", "TABLE_NOT_FOUND"),
            ("TableGridInvalid", "TABLE_GRID_INVALID"),
            ("CellNotFound", "CELL_NOT_FOUND"),
            ("LabelAmbiguous", "CELL_LABEL_AMBIGUOUS"),
            ("NonTextContent", "CELL_HAS_NON_TEXT_CONTENT"),
            ("TargetDuplicate", "CELL_TARGET_DUPLICATE"),
            ("TargetConflict", "CELL_TARGET_CONFLICT"),
            ("Codec", "SET_CELL_CODEC_FAILED"),
            ("NotRoundTripSafe", "INPUT_NOT_ROUNDTRIP_SAFE"),
            ("UncarriedZipEntries", "INPUT_ENTRIES_NOT_CARRIED"),
            ("SemanticLoss", "ENCODE_SEMANTIC_LOSS"),
        ],
    ),
    (
        "ReadError",
        &[
            ("Codec", "DECODE_FAILED"),
            ("SectionOutOfRange", "READ_SECTION_OUT_OF_RANGE"),
            ("ParaRangeInvalid", "READ_PARA_RANGE_INVALID"),
            ("TableOutOfRange", "READ_TABLE_OUT_OF_RANGE"),
            ("TableUnaddressable", "TABLE_GRID_INVALID"),
            ("FieldNotFound", "READ_FIELD_NOT_FOUND"),
        ],
    ),
    (
        "StructuralEditError",
        &[
            ("Codec", "STRUCTURAL_CODEC"),
            ("NotRoundTripSafe", "INPUT_NOT_ROUNDTRIP_SAFE"),
            ("UncarriedZipEntries", "INPUT_ENTRIES_NOT_CARRIED"),
            ("SectionOutOfRange", "SECTION_OUT_OF_RANGE"),
            ("ParagraphOutOfRange", "PARAGRAPH_OUT_OF_RANGE"),
            ("DuplicateTarget", "DUPLICATE_TARGET"),
            ("ReferenceStranded", "REFERENCE_STRANDED"),
            ("HardBreakLoss", "HARD_BREAK_LOSS"),
            ("EmptySection", "EMPTY_SECTION"),
            ("SectionPropertiesParagraph", "SECTION_PROPERTIES_PARAGRAPH"),
            ("SpanCountMismatch", "SPAN_COUNT_MISMATCH"),
            ("DeltaMismatch", "SELF_VERIFY_FAILED"),
            ("MultiParagraphText", "MULTI_PARAGRAPH_TEXT"),
            ("InsertBeforeSectionProperties", "INSERT_BEFORE_SECTION_PROPERTIES"),
        ],
    ),
    (
        "GridAddrError",
        &[
            ("ShapeMismatch", "GRID_ADDR_INVALID"),
            ("AddrMalformed", "GRID_ADDR_INVALID"),
            ("AddrMismatch", "GRID_ADDR_INVALID"),
            ("AddrOnUnaddressableTable", "GRID_ADDR_INVALID"),
        ],
    ),
    (
        "SectionWorkflowError",
        &[
            ("Decode", "DECODE_FAILED"),
            ("SectionOutOfRange", "SECTION_OUT_OF_RANGE"),
            ("SectionIndexMismatch", "SECTION_INDEX_MISMATCH"),
            ("PreservingPatch", "PATCH_FAILED"),
        ],
    ),
    (
        "StamperError",
        &[
            ("Codec", "STAMP_CODEC_FAILED"),
            ("NotRoundTripSafe", "INPUT_NOT_ROUNDTRIP_SAFE"),
            ("UncarriedZipEntries", "INPUT_ENTRIES_NOT_CARRIED"),
            ("Stamp", "STAMP_NAME_EMPTY"),
            ("ManifestInvariant", "STAMP_MANIFEST_INVARIANT"),
            ("SourceHashMismatch", "STAMP_SOURCE_HASH_MISMATCH"),
            ("CellStamp", "STAMP_NAME_EMPTY"),
            ("DeltaMismatch", "STAMP_DELTA_MISMATCH"),
            ("SemanticLoss", "ENCODE_SEMANTIC_LOSS"),
        ],
    ),
    (
        "StampError",
        &[
            ("UnknownSpec", "STAMP_SPEC_STALE"),
            ("MarkerMismatch", "STAMP_MARKER_MISMATCH"),
            ("DuplicateSpec", "STAMP_SPEC_DUPLICATE"),
            ("DuplicateName", "STAMP_NAME_DUPLICATE"),
            ("NameCollision", "STAMP_NAME_COLLISION"),
            ("UncoveredCandidate", "STAMP_CANDIDATE_UNCOVERED"),
            ("EmptyName", "STAMP_NAME_EMPTY"),
        ],
    ),
    (
        "CellStampError",
        &[
            ("TableNotFound", "TABLE_NOT_FOUND"),
            ("TableGridInvalid", "TABLE_GRID_INVALID"),
            ("NotAnAnchor", "STAMP_CELL_NOT_ANCHOR"),
            ("TargetNotStampable", "STAMP_CELL_NOT_EMPTY"),
            ("LabelDrift", "STAMP_LABEL_DRIFT"),
            ("UnknownCandidate", "STAMP_CELL_NOT_CANDIDATE"),
            ("DuplicateTarget", "STAMP_CELL_TARGET_DUPLICATE"),
            ("EmptyName", "STAMP_NAME_EMPTY"),
            ("BlankHint", "STAMP_HINT_BLANK"),
            ("DuplicateName", "STAMP_NAME_DUPLICATE"),
            ("NameCollision", "STAMP_NAME_COLLISION"),
            ("UncoveredCandidate", "STAMP_CANDIDATE_UNCOVERED"),
        ],
    ),
    (
        "StampMapError",
        &[
            ("Parse", "INVALID_STAMP_MAP"),
            ("UnsupportedShape", "INVALID_STAMP_MAP"),
            ("UnsupportedVersion", "INVALID_STAMP_MAP"),
            ("InvalidSourceHash", "INVALID_STAMP_MAP"),
            ("BlankHint", "INVALID_STAMP_MAP"),
        ],
    ),
    (
        "MdError",
        &[
            ("InvalidFrontmatter", "MD_DECODE_FAILED"),
            ("FrontmatterUnclosed", "MD_DECODE_FAILED"),
            ("TemplateResolution", "MD_DECODE_FAILED"),
            ("UnsupportedStructure", "MD_DECODE_FAILED"),
            ("OrphanNoteDefinition", "MD_DECODE_FAILED"),
            ("DuplicateNoteDefinition", "MD_DECODE_FAILED"),
            ("EmptyNoteDefinition", "MD_DECODE_FAILED"),
            ("NestedNoteReference", "MD_DECODE_FAILED"),
            ("NoteExpansionBudgetExceeded", "MD_DECODE_FAILED"),
            ("LosslessParse", "MD_DECODE_FAILED"),
            ("LosslessMissingAttribute", "MD_DECODE_FAILED"),
            ("LosslessInvalidAttribute", "MD_DECODE_FAILED"),
            ("FileTooLarge", "INPUT_TOO_LARGE"),
            ("AssetPlanMismatch", "ASSET_PLAN_MISMATCH"),
            ("AssetIdentityConflict", "ASSET_IDENTITY_CONFLICT"),
            ("Io", "MD_DECODE_FAILED"),
            ("Core", "MD_DECODE_FAILED"),
            ("Blueprint", "MD_DECODE_FAILED"),
            ("Foundation", "MD_DECODE_FAILED"),
        ],
    ),
];

// The tables above are the DECODE stage (the wrappers `OpsError::Decode` /
// `OpsError::MdDecode`). `encode_stage_reports_encode_failed` checks the
// other stage: every HWPX variant → `ENCODE_FAILED`, every Markdown variant
// → `ENCODE_FAILED` except the size and asset-contract codes, which are
// stage-independent.

fn foundation_errors() -> Vec<(&'static str, FoundationError)> {
    vec![
        ("InvalidHwpUnit", FoundationError::InvalidHwpUnit { value: 1 << 40, min: 0, max: 10 }),
        (
            "InvalidColor",
            FoundationError::InvalidColor { component: "red".into(), value: "300".into() },
        ),
        (
            "IndexOutOfBounds",
            FoundationError::IndexOutOfBounds { index: 9, max: 2, type_name: "FontIndex" },
        ),
        ("EmptyIdentifier", FoundationError::EmptyIdentifier { item: "FontId".into() }),
        (
            "InvalidField",
            FoundationError::InvalidField { field: "width".into(), reason: "negative".into() },
        ),
        (
            "ParseError",
            FoundationError::ParseError {
                type_name: "Alignment".into(),
                value: "sideways".into(),
                valid_values: "left, right".into(),
            },
        ),
    ]
}

const WARNING_MAPPING: Table = &[
    (
        "EncodeWarning",
        &[
            ("LayoutCacheDropped", "LAYOUT_CACHE_DROPPED"),
            ("NoteHeadSkipped", "NOTE_HEAD_SKIPPED"),
            ("TitleMarkSkipped", "TITLE_MARK_SKIPPED"),
            ("NoteRestartIgnored", "NOTE_RESTART_IGNORED"),
        ],
    ),
    (
        "DecodeWarning",
        &[
            ("UnknownEnumValue", "UNKNOWN_ENUM_VALUE"),
            ("LayoutCacheDropped", "LAYOUT_CACHE_DROPPED"),
        ],
    ),
    ("StructuralWarning", &[("IndexMarkRemoved", "INDEX_MARK_REMOVED")]),
    (
        "SectionWorkflowWarning",
        &[("PreservationMetadataUnavailable", "PRESERVATION_METADATA_UNAVAILABLE")],
    ),
    (
        "MdWarning",
        &[
            ("MergedCellsFlattened", "TABLE_MERGE_FLATTENED"),
            ("ImageEmbedSkipped", "IMAGE_EMBED_SKIPPED"),
        ],
    ),
    (
        "AssetOutcome",
        &[("Embedded", "ASSET_EMBEDDED"), ("Dropped", "ASSET_DROPPED"), ("Remote", "ASSET_REMOTE")],
    ),
];

// ── samples: one constructed value per variant ──────────────────

fn path() -> ParagraphPath {
    ParagraphPath(vec![PathSeg::Section(0)])
}

/// One semantic-loss encode warning, for the fail-closed error samples.
fn semantic_loss() -> EncodeWarning {
    EncodeWarning::NoteHeadSkipped { path: path(), reason: "titleMark first run".into() }
}

fn io_error() -> std::io::Error {
    std::io::Error::other("sample")
}

fn hwpx_errors() -> Vec<(&'static str, HwpxError)> {
    vec![
        ("Zip", HwpxError::Zip("bad central directory".into())),
        ("InvalidMimetype", HwpxError::InvalidMimetype { actual: "text/plain".into() }),
        ("MissingFile", HwpxError::MissingFile { path: "Contents/header.xml".into() }),
        ("XmlParse", HwpxError::XmlParse { file: "section0.xml".into(), detail: "eof".into() }),
        (
            "InvalidAttribute",
            HwpxError::InvalidAttribute {
                element: "hp:p".into(),
                attribute: "paraPrIDRef".into(),
                value: "x".into(),
            },
        ),
        ("IndexOutOfBounds", HwpxError::IndexOutOfBounds { kind: "charPrIDRef", index: 9, max: 2 }),
        ("InvalidStructure", HwpxError::InvalidStructure { detail: "no sections".into() }),
        (
            "LayoutCacheDropped",
            HwpxError::LayoutCacheDropped { path: "§0/p3".into(), reason: "ledger".into() },
        ),
        ("Io", HwpxError::Io(io_error())),
        ("Core", HwpxError::Core(CoreError::Validation(ValidationError::EmptyDocument))),
        (
            "Foundation",
            HwpxError::Foundation(FoundationError::EmptyIdentifier { item: "FontId".into() }),
        ),
        ("XmlSerialize", HwpxError::XmlSerialize { detail: "writer".into() }),
    ]
}

fn core_errors() -> Vec<(&'static str, CoreError)> {
    vec![
        ("Validation", CoreError::Validation(ValidationError::EmptyDocument)),
        (
            "Foundation",
            CoreError::Foundation(FoundationError::EmptyIdentifier { item: "x".into() }),
        ),
        (
            "InvalidStructure",
            CoreError::InvalidStructure { context: "section 0".into(), reason: "empty".into() },
        ),
    ]
}

fn fill_errors() -> Vec<(&'static str, FillError)> {
    vec![
        ("EmptyValue", FillError::EmptyValue { name: "제목".into() }),
        ("UnknownField", FillError::UnknownField { name: "x".into(), available: vec![] }),
        ("DuplicateFieldName", FillError::DuplicateFieldName { name: "x".into(), count: 2 }),
        ("UnfillableField", FillError::UnfillableField { name: "x".into(), section: 0 }),
        ("Workflow", FillError::Workflow(HwpxError::Zip("z".into()))),
    ]
}

fn cell_edit_errors() -> Vec<(&'static str, CellEditError)> {
    let at = GridCoord::new(0, 0);
    vec![
        ("TableNotFound", CellEditError::TableNotFound { table: 3, tables: 1 }),
        ("TableGridInvalid", CellEditError::TableGridInvalid { table: 0, reason: "span".into() }),
        (
            "CellNotFound",
            CellEditError::CellNotFound { table: 0, detail: "no".into(), candidates: vec![] },
        ),
        (
            "LabelAmbiguous",
            CellEditError::LabelAmbiguous { table: 0, label: "성명".into(), count: 2 },
        ),
        ("NonTextContent", CellEditError::NonTextContent { table: 0, anchor: at }),
        ("TargetDuplicate", CellEditError::TargetDuplicate { table: 0, anchor: at }),
        ("TargetConflict", CellEditError::TargetConflict { outer_table: 0, inner_table: 1 }),
        ("Codec", CellEditError::Codec("encode".into())),
        (
            "NotRoundTripSafe",
            CellEditError::NotRoundTripSafe { component: "core".into(), diff_path: "/a".into() },
        ),
        ("UncarriedZipEntries", CellEditError::UncarriedZipEntries { entries: vec!["x".into()] }),
        (
            "SemanticLoss",
            CellEditError::SemanticLoss { warnings: vec![semantic_loss()], others: vec![] },
        ),
    ]
}

fn read_errors() -> Vec<(&'static str, ReadError)> {
    vec![
        ("Codec", ReadError::Codec(HwpxError::Zip("z".into()))),
        ("SectionOutOfRange", ReadError::SectionOutOfRange { requested: 2, available: 1 }),
        (
            "ParaRangeInvalid",
            ReadError::ParaRangeInvalid { section: 0, from: 5, to: 9, available: 2 },
        ),
        ("TableOutOfRange", ReadError::TableOutOfRange { requested: 3, available: 1 }),
        ("TableUnaddressable", ReadError::TableUnaddressable { ordinal: 0, reason: "span".into() }),
        ("FieldNotFound", ReadError::FieldNotFound { name: "x".into(), available: vec![] }),
    ]
}

fn structural_errors() -> Vec<(&'static str, StructuralEditError)> {
    vec![
        ("Codec", StructuralEditError::Codec("encode".into())),
        (
            "NotRoundTripSafe",
            StructuralEditError::NotRoundTripSafe {
                component: "core".into(),
                diff_path: "/a".into(),
            },
        ),
        (
            "UncarriedZipEntries",
            StructuralEditError::UncarriedZipEntries { entries: vec!["x".into()] },
        ),
        ("SectionOutOfRange", StructuralEditError::SectionOutOfRange { section: 2, available: 1 }),
        (
            "ParagraphOutOfRange",
            StructuralEditError::ParagraphOutOfRange { section: 0, index: 9, available: 2 },
        ),
        ("DuplicateTarget", StructuralEditError::DuplicateTarget { section: 0, index: 1 }),
        ("ReferenceStranded", StructuralEditError::ReferenceStranded { section: 0, index: 1 }),
        ("HardBreakLoss", StructuralEditError::HardBreakLoss { section: 0, index: 1 }),
        ("EmptySection", StructuralEditError::EmptySection { section: 0 }),
        (
            "SectionPropertiesParagraph",
            StructuralEditError::SectionPropertiesParagraph { section: 0, index: 0 },
        ),
        (
            "SpanCountMismatch",
            StructuralEditError::SpanCountMismatch { section: 0, decoded: 3, wire: 4 },
        ),
        ("DeltaMismatch", StructuralEditError::DeltaMismatch { detail: "text".into() }),
        ("MultiParagraphText", StructuralEditError::MultiParagraphText),
        (
            "InsertBeforeSectionProperties",
            StructuralEditError::InsertBeforeSectionProperties { section: 0, index: 0 },
        ),
    ]
}

fn grid_addr_errors() -> Vec<(&'static str, GridAddrError)> {
    vec![
        ("ShapeMismatch", GridAddrError::ShapeMismatch { path: "/a".into() }),
        ("AddrMalformed", GridAddrError::AddrMalformed { path: "/a".into() }),
        (
            "AddrMismatch",
            GridAddrError::AddrMismatch {
                path: "/a".into(),
                supplied: GridCoord::new(0, 0),
                expected: GridCoord::new(1, 0),
            },
        ),
        (
            "AddrOnUnaddressableTable",
            GridAddrError::AddrOnUnaddressableTable { path: "/a".into(), reason: "span".into() },
        ),
    ]
}

fn section_workflow_errors() -> Vec<(&'static str, SectionWorkflowError)> {
    vec![
        ("Decode", SectionWorkflowError::Decode { detail: "zip".into() }),
        (
            "SectionOutOfRange",
            SectionWorkflowError::SectionOutOfRange { requested: 2, sections: 1 },
        ),
        (
            "SectionIndexMismatch",
            SectionWorkflowError::SectionIndexMismatch { requested: 0, actual: 1 },
        ),
        ("PreservingPatch", SectionWorkflowError::PreservingPatch(HwpxError::Zip("z".into()))),
    ]
}

fn stamper_errors() -> Vec<(&'static str, StamperError)> {
    vec![
        ("Codec", StamperError::Codec("encode".into())),
        (
            "NotRoundTripSafe",
            StamperError::NotRoundTripSafe { component: "core".into(), diff_path: "/a".into() },
        ),
        ("UncarriedZipEntries", StamperError::UncarriedZipEntries { entries: vec!["x".into()] }),
        ("Stamp", StamperError::Stamp(StampError::EmptyName)),
        ("ManifestInvariant", StamperError::ManifestInvariant { detail: "count".into() }),
        (
            "SourceHashMismatch",
            StamperError::SourceHashMismatch { expected: "a".into(), actual: "b".into() },
        ),
        ("CellStamp", StamperError::CellStamp(CellStampError::EmptyName)),
        (
            "DeltaMismatch",
            StamperError::DeltaMismatch { stage: "post".into(), detail: "text".into() },
        ),
        (
            "SemanticLoss",
            StamperError::SemanticLoss { warnings: vec![semantic_loss()], others: vec![] },
        ),
    ]
}

fn stamp_errors() -> Vec<(&'static str, StampError)> {
    let span: Range<usize> = 0..3;
    vec![
        (
            "UnknownSpec",
            StampError::UnknownSpec { section: 0, path: "/a".into(), span: span.clone() },
        ),
        (
            "MarkerMismatch",
            StampError::MarkerMismatch {
                path: "/a".into(),
                expected: "(  )".into(),
                found: "( )".into(),
            },
        ),
        ("DuplicateSpec", StampError::DuplicateSpec { path: "/a".into(), span: span.clone() }),
        ("DuplicateName", StampError::DuplicateName { name: "성명".into() }),
        ("NameCollision", StampError::NameCollision { name: "성명".into() }),
        (
            "UncoveredCandidate",
            StampError::UncoveredCandidate {
                section: 0,
                path: "/a".into(),
                span,
                marker: "(  )".into(),
            },
        ),
        ("EmptyName", StampError::EmptyName),
    ]
}

fn cell_stamp_errors() -> Vec<(&'static str, CellStampError)> {
    let at = GridCoord::new(1, 2);
    vec![
        ("TableNotFound", CellStampError::TableNotFound { table: 9 }),
        ("TableGridInvalid", CellStampError::TableGridInvalid { table: 0, detail: "span".into() }),
        ("NotAnAnchor", CellStampError::NotAnAnchor { table: 0, requested: at, anchor: Some(at) }),
        ("TargetNotStampable", CellStampError::TargetNotStampable { table: 0, at }),
        (
            "LabelDrift",
            CellStampError::LabelDrift {
                table: 0,
                at,
                claimed: "성명".into(),
                found: Some("이름".into()),
            },
        ),
        ("UnknownCandidate", CellStampError::UnknownCandidate { table: 0, at }),
        ("DuplicateTarget", CellStampError::DuplicateTarget { table: 0, at }),
        ("EmptyName", CellStampError::EmptyName),
        ("BlankHint", CellStampError::BlankHint { name: "성명".into() }),
        ("DuplicateName", CellStampError::DuplicateName { name: "성명".into() }),
        ("NameCollision", CellStampError::NameCollision { name: "성명".into() }),
        ("UncoveredCandidate", CellStampError::UncoveredCandidate { table: 0, at }),
    ]
}

fn stamp_map_errors() -> Vec<(&'static str, StampMapError)> {
    vec![
        ("Parse", StampMapError::Parse("unknown field".into())),
        ("UnsupportedShape", StampMapError::UnsupportedShape),
        ("UnsupportedVersion", StampMapError::UnsupportedVersion(99)),
        ("InvalidSourceHash", StampMapError::InvalidSourceHash("zz".into())),
        ("BlankHint", StampMapError::BlankHint { name: "성명".into() }),
    ]
}

fn md_errors() -> Vec<(&'static str, MdError)> {
    let occurrence = RunLocator::new(0, 0);
    vec![
        ("InvalidFrontmatter", MdError::InvalidFrontmatter { detail: "yaml".into() }),
        ("FrontmatterUnclosed", MdError::FrontmatterUnclosed),
        ("TemplateResolution", MdError::TemplateResolution { detail: "missing".into() }),
        ("UnsupportedStructure", MdError::UnsupportedStructure { detail: "nested".into() }),
        ("OrphanNoteDefinition", MdError::OrphanNoteDefinition { label: "1".into() }),
        ("DuplicateNoteDefinition", MdError::DuplicateNoteDefinition { label: "1".into() }),
        ("EmptyNoteDefinition", MdError::EmptyNoteDefinition { label: "1".into() }),
        ("NestedNoteReference", MdError::NestedNoteReference { label: "1".into() }),
        ("NoteExpansionBudgetExceeded", MdError::NoteExpansionBudgetExceeded { budget: 64 }),
        ("LosslessParse", MdError::LosslessParse { detail: "html".into() }),
        (
            "LosslessMissingAttribute",
            MdError::LosslessMissingAttribute { element: "td", attribute: "colspan" },
        ),
        (
            "LosslessInvalidAttribute",
            MdError::LosslessInvalidAttribute {
                element: "td",
                attribute: "colspan",
                value: "x".into(),
            },
        ),
        ("FileTooLarge", MdError::FileTooLarge { size: 1, limit: 0 }),
        ("AssetPlanMismatch", MdError::AssetPlanMismatch { occurrence, detail: "extra".into() }),
        (
            "AssetIdentityConflict",
            MdError::AssetIdentityConflict { occurrence, identity: "a.png".into() },
        ),
        ("Io", MdError::Io(io_error())),
        ("Core", MdError::Core(CoreError::Validation(ValidationError::EmptyDocument))),
        (
            "Blueprint",
            MdError::Blueprint(hwpforge::blueprint::error::BlueprintError::TemplateNotFound {
                name: "x".into(),
            }),
        ),
        ("Foundation", MdError::Foundation(FoundationError::EmptyIdentifier { item: "x".into() })),
    ]
}

fn wrapped_errors() -> BTreeMap<&'static str, Vec<(&'static str, OpsError)>> {
    let mut all: BTreeMap<&'static str, Vec<(&'static str, OpsError)>> = BTreeMap::new();
    all.insert(
        "HwpxError",
        hwpx_errors().into_iter().map(|(n, e)| (n, OpsError::decode(e))).collect(),
    );
    all.insert("CoreError", core_errors().into_iter().map(|(n, e)| (n, e.into())).collect());
    all.insert(
        "FoundationError",
        foundation_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert("FillError", fill_errors().into_iter().map(|(n, e)| (n, e.into())).collect());
    all.insert(
        "CellEditError",
        cell_edit_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert("ReadError", read_errors().into_iter().map(|(n, e)| (n, e.into())).collect());
    all.insert(
        "StructuralEditError",
        structural_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert(
        "GridAddrError",
        grid_addr_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert(
        "SectionWorkflowError",
        section_workflow_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert("StamperError", stamper_errors().into_iter().map(|(n, e)| (n, e.into())).collect());
    all.insert(
        "StampError",
        stamp_errors()
            .into_iter()
            .map(|(n, e)| (n, OpsError::Stamper(StamperError::Stamp(e))))
            .collect(),
    );
    all.insert(
        "CellStampError",
        cell_stamp_errors()
            .into_iter()
            .map(|(n, e)| (n, OpsError::Stamper(StamperError::CellStamp(e))))
            .collect(),
    );
    all.insert(
        "StampMapError",
        stamp_map_errors().into_iter().map(|(n, e)| (n, e.into())).collect(),
    );
    all.insert(
        "MdError",
        md_errors().into_iter().map(|(n, e)| (n, OpsError::md_decode(e))).collect(),
    );
    all
}

fn wrapped_warnings() -> BTreeMap<&'static str, Vec<(&'static str, OpsWarning)>> {
    let mut all: BTreeMap<&'static str, Vec<(&'static str, OpsWarning)>> = BTreeMap::new();
    all.insert(
        "EncodeWarning",
        vec![
            (
                "LayoutCacheDropped",
                OpsWarning::Encode(EncodeWarning::LayoutCacheDropped {
                    path: path(),
                    reason: "ledger".into(),
                }),
            ),
            (
                "NoteHeadSkipped",
                OpsWarning::Encode(EncodeWarning::NoteHeadSkipped {
                    path: path(),
                    reason: "titleMark".into(),
                }),
            ),
            (
                "TitleMarkSkipped",
                OpsWarning::Encode(EncodeWarning::TitleMarkSkipped {
                    path: path(),
                    reason: "placeholder".into(),
                }),
            ),
            (
                "NoteRestartIgnored",
                OpsWarning::Encode(EncodeWarning::NoteRestartIgnored {
                    path: path(),
                    reason: "ON_SECTION".into(),
                }),
            ),
        ],
    );
    all.insert(
        "SectionWorkflowWarning",
        vec![(
            "PreservationMetadataUnavailable",
            OpsWarning::SectionWorkflow(SectionWorkflowWarning::PreservationMetadataUnavailable {
                detail: "no linesegarray".into(),
            }),
        )],
    );
    all.insert(
        "StructuralWarning",
        vec![(
            "IndexMarkRemoved",
            OpsWarning::Structural(StructuralWarning::IndexMarkRemoved {
                section: 0,
                index: 3,
                count: 2,
            }),
        )],
    );
    all.insert(
        "DecodeWarning",
        vec![
            (
                "UnknownEnumValue",
                OpsWarning::Decode(DecodeWarning::UnknownEnumValue {
                    attribute: "hp:header@applyPageType",
                    raw: "WEIRD".into(),
                    fallback: "BOTH",
                }),
            ),
            (
                "LayoutCacheDropped",
                OpsWarning::Decode(DecodeWarning::LayoutCacheDropped {
                    path: path(),
                    reason: "textpos".into(),
                }),
            ),
        ],
    );
    all.insert(
        "MdWarning",
        vec![
            (
                "MergedCellsFlattened",
                OpsWarning::Md(MdWarning::MergedCellsFlattened { merged_cells: 2 }),
            ),
            (
                "ImageEmbedSkipped",
                OpsWarning::Md(MdWarning::ImageEmbedSkipped {
                    src: "a.png".into(),
                    reason: ImageEmbedSkipReason::MissingFile,
                }),
            ),
        ],
    );
    all.insert(
        "AssetOutcome",
        vec![
            (
                "Embedded",
                OpsWarning::Asset(AssetOutcome::Embedded {
                    occurrence: RunLocator::new(0, 0),
                    key: "image1.png".into(),
                    format: ImageFormat::Png,
                }),
            ),
            (
                "Dropped",
                OpsWarning::Asset(AssetOutcome::Dropped {
                    occurrence: RunLocator::new(0, 1),
                    reason: ImageEmbedSkipReason::PathEscapes,
                }),
            ),
            (
                "Remote",
                OpsWarning::Asset(AssetOutcome::Remote { occurrence: RunLocator::new(1, 0) }),
            ),
        ],
    );
    all
}

// ── the tests ───────────────────────────────────────────────────

#[test]
fn inventory_matches_the_tracked_file() {
    let regenerated = error_inventory::collect();
    let rendered = serde_json::to_string_pretty(&regenerated).expect("serialise") + "\n";

    if std::env::var_os("UPDATE_INVENTORY").is_some() {
        std::fs::write(TRACKED, &rendered).expect("write inventory");
        return;
    }

    let tracked = std::fs::read_to_string(TRACKED).expect("tracked inventory missing");
    if tracked != rendered {
        let stored: error_inventory::Inventory =
            serde_json::from_str(&tracked).expect("tracked inventory does not parse");
        let mut diff = Vec::new();
        for fresh in &regenerated.enums {
            match stored.enums.iter().find(|e| e.name == fresh.name) {
                None => diff.push(format!("+ enum {}", fresh.name)),
                Some(old) if old != fresh => diff.push(format!(
                    "~ enum {}: {:?} -> {:?}",
                    fresh.name,
                    old.variants.iter().map(|v| &v.name).collect::<Vec<_>>(),
                    fresh.variants.iter().map(|v| &v.name).collect::<Vec<_>>()
                )),
                Some(_) => {}
            }
        }
        for old in &stored.enums {
            if !regenerated.enums.iter().any(|e| e.name == old.name) {
                diff.push(format!("- enum {}", old.name));
            }
        }
        panic!(
            "upstream error/warning inventory changed:\n{}\n\
             classify the new variants in `hwpforge::ops`, then rerun with \
             UPDATE_INVENTORY=1 to rewrite tests/data/ops_error_inventory.json",
            if diff.is_empty() { "(formatting only)".to_owned() } else { diff.join("\n") }
        );
    }
}

#[test]
fn every_wrapped_payload_type_has_a_manifest_record() {
    // The payload types are read from `OpsError`/`OpsWarning` themselves (not
    // from a hand-kept list), so adding an arm without extending the audit
    // scope — or without a mapping table — fails here instead of going unseen.
    let manifest_wrapped: BTreeSet<&str> =
        error_inventory::MANIFEST.iter().flat_map(|e| e.wrapped.iter().copied()).collect();
    let table_names: BTreeSet<&str> =
        ERROR_MAPPING.iter().chain(WARNING_MAPPING.iter()).map(|(name, _)| *name).collect();
    // Not audited as enums: `serde_json::Error` is external and classified by
    // stage; `GridAddrWarning` is a struct.
    let not_enums: BTreeSet<&str> = ["Error", "GridAddrWarning"].into_iter().collect();
    // Reached through another wrapped enum rather than as a direct payload.
    let nested: BTreeSet<&str> = ["StampError", "CellStampError"].into_iter().collect();

    let derived = error_inventory::wrapped_payloads_of_ops();
    let derived: BTreeSet<&str> =
        derived.iter().map(String::as_str).filter(|n| !not_enums.contains(n)).collect();

    let not_in_manifest: Vec<&str> = derived.difference(&manifest_wrapped).copied().collect();
    assert!(
        not_in_manifest.is_empty(),
        "wrapped by ops but absent from MANIFEST.wrapped: {not_in_manifest:?}"
    );
    let not_in_tables: Vec<&str> = derived.difference(&table_names).copied().collect();
    assert!(
        not_in_tables.is_empty(),
        "wrapped by ops but without a mapping table: {not_in_tables:?}"
    );
    let stale: Vec<&str> = table_names
        .iter()
        .copied()
        .filter(|n| !derived.contains(n) && !nested.contains(n))
        .collect();
    assert!(stale.is_empty(), "mapping table for a type ops no longer wraps: {stale:?}");
    for n in &nested {
        assert!(
            manifest_wrapped.contains(n) && table_names.contains(n),
            "nested {n} must stay audited"
        );
    }
}

#[test]
fn manifest_files_hold_no_unnamed_diagnostic_enum() {
    let inventory = error_inventory::collect();

    assert!(
        inventory.unlisted_public_error_or_warning_enums.is_empty(),
        "public *Error/*Warning enums outside the manifest: {:?} — add them to \
         MANIFEST (wrapped, or not_wrapped with a reason)",
        inventory.unlisted_public_error_or_warning_enums
    );
}

#[test]
fn every_wrapped_variant_has_a_mapping() {
    let inventory = error_inventory::collect();
    let tables: Vec<&(&str, &[(&str, &str)])> =
        ERROR_MAPPING.iter().chain(WARNING_MAPPING.iter()).collect();

    for record in inventory.enums.iter().filter(|e| e.wrapped) {
        let table = tables
            .iter()
            .find(|(name, _)| *name == record.name)
            .unwrap_or_else(|| panic!("{} is wrapped but has no mapping table", record.name));

        let inventoried: Vec<&str> = record.variants.iter().map(|v| v.name.as_str()).collect();
        let mapped: Vec<&str> = table.1.iter().map(|(variant, _)| *variant).collect();
        assert_eq!(
            inventoried, mapped,
            "{}: inventoried variants and the mapping table disagree",
            record.name
        );
    }

    let wrapped: Vec<&str> =
        inventory.enums.iter().filter(|e| e.wrapped).map(|e| e.name.as_str()).collect();
    for (name, _) in &tables {
        assert!(wrapped.contains(name), "{name} has a mapping table but is not in the manifest");
    }
}

#[test]
fn constructed_errors_produce_the_mapped_code() {
    let samples = wrapped_errors();

    for (enum_name, table) in ERROR_MAPPING {
        let built = samples
            .get(enum_name)
            .unwrap_or_else(|| panic!("{enum_name} has a table but no sample values"));
        let built_names: Vec<&str> = built.iter().map(|(name, _)| *name).collect();
        let table_names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
        assert_eq!(built_names, table_names, "{enum_name}: samples and table disagree");

        for ((variant, expected), (_, error)) in table.iter().zip(built) {
            assert_eq!(
                error.code().as_str(),
                *expected,
                "{enum_name}::{variant} classified as {}",
                error.code()
            );
        }
    }
}

#[test]
fn constructed_warnings_produce_the_mapped_code() {
    let samples = wrapped_warnings();

    for (enum_name, table) in WARNING_MAPPING {
        let built = samples
            .get(enum_name)
            .unwrap_or_else(|| panic!("{enum_name} has a table but no sample values"));
        let built_names: Vec<&str> = built.iter().map(|(name, _)| *name).collect();
        let table_names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
        assert_eq!(built_names, table_names, "{enum_name}: samples and table disagree");

        for ((variant, expected), (_, warning)) in table.iter().zip(built) {
            let info = warning.info();
            assert_eq!(info.code, *expected, "{enum_name}::{variant}");
            assert!(!info.message.is_empty(), "{enum_name}::{variant} has no message");
        }
    }
}

#[test]
fn asset_contract_violations_get_dedicated_codes() {
    // W1a adversarial review F6: an asset reply that does not match the plan,
    // or two assets claiming one identity with different bytes, is the caller
    // supplying the wrong thing — and each gets its own code so the caller can
    // tell which contract broke.
    let occurrence = RunLocator::new(2, 1);
    let cases = [
        (
            MdError::AssetPlanMismatch { occurrence, detail: "no plan entry".into() },
            OpsCode::AssetPlanMismatch,
            "ASSET_PLAN_MISMATCH",
        ),
        (
            MdError::AssetIdentityConflict { occurrence, identity: "logo.png".into() },
            OpsCode::AssetIdentityConflict,
            "ASSET_IDENTITY_CONFLICT",
        ),
    ];

    for (error, code, wire) in cases {
        let classified = OpsError::md_decode(error);
        assert_eq!(classified.code(), code, "{classified}");
        assert_eq!(classified.code().as_str(), wire);
    }
}

#[test]
fn the_stage_wins_over_a_nested_core_error() {
    // The CLI reports `DECODE_FAILED` for anything the decoder returns and
    // `VALIDATION_FAILED` only for a direct `validate()` failure; a Core
    // error nested inside a codec error therefore takes the stage's code.
    for (variant, error) in core_errors() {
        let direct = OpsError::Core(error);
        let expected = match variant {
            "Foundation" => OpsCode::InternalInvariant,
            _ => OpsCode::ValidationFailed,
        };
        assert_eq!(direct.code(), expected, "direct CoreError::{variant}");
    }
    for (variant, error) in core_errors() {
        let nested = OpsError::decode(HwpxError::Core(error));
        assert_eq!(nested.code(), OpsCode::DecodeFailed, "decode-nested CoreError::{variant}");
    }
    for (variant, error) in core_errors() {
        let nested = OpsError::encode(HwpxError::Core(error));
        assert_eq!(nested.code(), OpsCode::EncodeFailed, "encode-nested CoreError::{variant}");
    }
}

#[test]
fn grid_addr_projection_reports_its_own_code() {
    // Review-driven split (lane A): the CLI prints `GRID_ADDR_PROJECTION_FAILED`
    // for an annotate failure and `GRID_ADDR_INVALID` for a verify failure.
    for (variant, error) in grid_addr_errors() {
        assert_eq!(
            OpsError::grid_addr_projection(error).code(),
            OpsCode::GridAddrProjectionFailed,
            "GridAddrError::{variant}"
        );
    }
}

#[test]
fn encode_stage_reports_encode_failed() {
    for (variant, error) in hwpx_errors() {
        assert_eq!(OpsError::encode(error).code(), OpsCode::EncodeFailed, "HwpxError::{variant}");
    }
    for (variant, error) in md_errors() {
        let expected = match variant {
            "FileTooLarge" => OpsCode::InputTooLarge,
            "AssetPlanMismatch" => OpsCode::AssetPlanMismatch,
            "AssetIdentityConflict" => OpsCode::AssetIdentityConflict,
            _ => OpsCode::EncodeFailed,
        };
        assert_eq!(OpsError::md_encode(error).code(), expected, "MdError::{variant}");
    }
}

#[test]
fn semantic_loss_variants_become_the_uniform_envelope() {
    // Review R2 #2: set-cell and stamp must fail closed with the same
    // structured envelope restyle uses, not a wrapped upstream variant.
    let cell: OpsError = CellEditError::SemanticLoss {
        warnings: vec![semantic_loss()],
        others: vec![EncodeWarning::LayoutCacheDropped { path: path(), reason: "ledger".into() }],
    }
    .into();
    let stamp: OpsError =
        StamperError::SemanticLoss { warnings: vec![semantic_loss()], others: Vec::new() }.into();
    for (label, error) in [("cell", cell), ("stamp", stamp)] {
        let OpsError::EncodeSemanticLoss { warnings, others } = &error else {
            panic!("{label}: expected EncodeSemanticLoss, got {error:?}");
        };
        assert_eq!(warnings.len(), 1, "{label}");
        assert_eq!(warnings[0].code, "NOTE_HEAD_SKIPPED", "{label}");
        assert_eq!(error.code(), OpsCode::EncodeSemanticLoss, "{label}");
        if label == "cell" {
            assert_eq!(others.len(), 1);
            assert_eq!(others[0].code, "LAYOUT_CACHE_DROPPED");
        }
    }
}

#[test]
fn one_code_never_offers_two_different_hints() {
    // Today `hint()` is `hint_for(self.code())`, so this holds by
    // construction. It is here as a guard for the refactor that would break
    // it: if a hint ever became per-variant, two errors sharing a code could
    // start printing different advice, and this test is what would catch it.
    let mut hints_per_code: BTreeMap<&str, BTreeSet<Option<&'static str>>> = BTreeMap::new();
    for (_, errors) in wrapped_errors() {
        for (_, error) in errors {
            hints_per_code.entry(error.code().as_str()).or_default().insert(error.hint());
        }
    }

    for (code, hints) in &hints_per_code {
        assert_eq!(hints.len(), 1, "{code} offers {} different hints: {hints:?}", hints.len());
    }
    assert!(hints_per_code.len() > 20, "the samples should cover most of the code table");
}
