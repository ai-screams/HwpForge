//! Error types for the Markdown Smithy.

use std::fmt;

/// Top-level error type for smithy-md operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MdError {
    /// YAML frontmatter exists but failed to parse.
    #[error("invalid YAML frontmatter: {detail}")]
    InvalidFrontmatter {
        /// Parsing error details.
        detail: String,
    },

    /// The document started a frontmatter block but never closed it.
    #[error("frontmatter block started with '---' but no closing marker was found")]
    FrontmatterUnclosed,

    /// Template inheritance could not be resolved with available providers.
    #[error("template resolution failed: {detail}")]
    TemplateResolution {
        /// Resolution error details.
        detail: String,
    },

    /// The markdown content contains a structure this decoder cannot map.
    #[error("unsupported markdown structure: {detail}")]
    UnsupportedStructure {
        /// Unsupported structure details.
        detail: String,
    },

    /// A footnote/endnote definition never referenced from the body.
    #[error("orphan note definition '[^{label}]' — defined but never referenced")]
    OrphanNoteDefinition {
        /// Original definition label.
        label: String,
    },

    /// The same note label was defined more than once.
    #[error("duplicate note definition '[^{label}]' — the body is ambiguous")]
    DuplicateNoteDefinition {
        /// Original definition label.
        label: String,
    },

    /// A note definition with no content.
    #[error("empty note definition '[^{label}]' — a note needs body text")]
    EmptyNoteDefinition {
        /// Original definition label.
        label: String,
    },

    /// A note reference inside another note's definition (HWP cannot nest notes).
    #[error("nested note reference '[^{label}]' inside a note definition")]
    NestedNoteReference {
        /// Referenced label.
        label: String,
    },

    /// Total expanded note content exceeded the safety budget.
    #[error("expanded note content exceeds the {budget}-byte budget (duplicated references)")]
    NoteExpansionBudgetExceeded {
        /// Budget in bytes.
        budget: usize,
    },

    /// Lossless body parsing failed.
    #[error("invalid lossless body: {detail}")]
    LosslessParse {
        /// Parsing error details.
        detail: String,
    },

    /// Required attribute is missing in a lossless element.
    #[error("missing required attribute '{attribute}' on <{element}>")]
    LosslessMissingAttribute {
        /// Element name.
        element: &'static str,
        /// Missing attribute name.
        attribute: &'static str,
    },

    /// Attribute value in a lossless element is invalid.
    #[error("invalid attribute '{attribute}' on <{element}>: {value}")]
    LosslessInvalidAttribute {
        /// Element name.
        element: &'static str,
        /// Attribute name.
        attribute: &'static str,
        /// Invalid value.
        value: String,
    },

    /// Input file exceeds the maximum allowed size.
    #[error("file too large: {size} bytes exceeds {limit} byte limit")]
    FileTooLarge {
        /// Actual file size in bytes.
        size: u64,
        /// Maximum allowed size in bytes.
        limit: u64,
    },

    /// The provided asset set does not match the plan collected from the document.
    ///
    /// Raised by [`crate::assets::finish_assets`] when an occurrence is
    /// unknown, provided twice, provided for a non-`File` source, or when a
    /// `File` occurrence was never provided. It also fires when the document
    /// drifted from the plan the caller provisioned against — the plan is
    /// re-collected and compared entry by entry, so a source swapped behind
    /// unchanged locators is caught rather than embedded into the wrong run.
    #[error("asset plan mismatch at {occurrence}: {detail}")]
    AssetPlanMismatch {
        /// Image run the mismatch was detected at.
        occurrence: crate::assets::RunLocator,
        /// Which rule was violated. Contract breaches use a fixed phrase;
        /// plan drift names the first differing index and both entries.
        detail: String,
    },

    /// The same asset identity was provided with two different byte sequences.
    #[error("asset identity conflict at {occurrence}: {identity} was provided with two different byte sequences")]
    AssetIdentityConflict {
        /// Image run where the second, differing provision appeared.
        occurrence: crate::assets::RunLocator,
        /// Display form of the conflicting identity (truncated).
        identity: String,
    },

    /// I/O error for file convenience APIs.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Core-layer error propagated upward.
    #[error("core error: {0}")]
    Core(#[from] hwpforge_core::CoreError),

    /// Blueprint-layer error propagated upward.
    #[error("blueprint error: {0}")]
    Blueprint(#[from] hwpforge_blueprint::error::BlueprintError),

    /// Foundation-layer error propagated upward.
    #[error("foundation error: {0}")]
    Foundation(#[from] hwpforge_foundation::FoundationError),
}

/// Error codes for smithy-md (6000-6999 range).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MdErrorCode {
    /// Invalid YAML frontmatter.
    InvalidFrontmatter = 6000,
    /// Frontmatter delimiter was not closed.
    FrontmatterUnclosed = 6001,
    /// Template inheritance resolution failed.
    TemplateResolution = 6002,
    /// Unsupported markdown structure.
    UnsupportedStructure = 6003,
    /// Invalid lossless body.
    LosslessParse = 6008,
    /// Missing lossless element attribute.
    LosslessMissingAttribute = 6009,
    /// Invalid lossless element attribute value.
    LosslessInvalidAttribute = 6010,
    /// File exceeds maximum allowed size.
    FileTooLarge = 6011,
    /// I/O failure.
    Io = 6004,
    /// Propagated Core error.
    Core = 6005,
    /// Propagated Blueprint error.
    Blueprint = 6006,
    /// Propagated Foundation error.
    Foundation = 6007,
    /// Orphan note definition (never referenced).
    OrphanNoteDefinition = 6012,
    /// Duplicate note definition label.
    DuplicateNoteDefinition = 6013,
    /// Empty note definition.
    EmptyNoteDefinition = 6014,
    /// Nested note reference inside a definition.
    NestedNoteReference = 6015,
    /// Expanded note content exceeded budget.
    NoteExpansionBudgetExceeded = 6016,
    /// Provided assets do not match the document's asset plan.
    AssetPlanMismatch = 6017,
    /// One asset identity carried two different byte sequences.
    AssetIdentityConflict = 6018,
}

impl fmt::Display for MdErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{}", *self as u16)
    }
}

impl MdError {
    /// Returns the corresponding stable error code.
    pub fn code(&self) -> MdErrorCode {
        match self {
            Self::InvalidFrontmatter { .. } => MdErrorCode::InvalidFrontmatter,
            Self::FrontmatterUnclosed => MdErrorCode::FrontmatterUnclosed,
            Self::TemplateResolution { .. } => MdErrorCode::TemplateResolution,
            Self::UnsupportedStructure { .. } => MdErrorCode::UnsupportedStructure,
            Self::OrphanNoteDefinition { .. } => MdErrorCode::OrphanNoteDefinition,
            Self::DuplicateNoteDefinition { .. } => MdErrorCode::DuplicateNoteDefinition,
            Self::EmptyNoteDefinition { .. } => MdErrorCode::EmptyNoteDefinition,
            Self::NestedNoteReference { .. } => MdErrorCode::NestedNoteReference,
            Self::NoteExpansionBudgetExceeded { .. } => MdErrorCode::NoteExpansionBudgetExceeded,
            Self::LosslessParse { .. } => MdErrorCode::LosslessParse,
            Self::LosslessMissingAttribute { .. } => MdErrorCode::LosslessMissingAttribute,
            Self::LosslessInvalidAttribute { .. } => MdErrorCode::LosslessInvalidAttribute,
            Self::FileTooLarge { .. } => MdErrorCode::FileTooLarge,
            Self::AssetPlanMismatch { .. } => MdErrorCode::AssetPlanMismatch,
            Self::AssetIdentityConflict { .. } => MdErrorCode::AssetIdentityConflict,
            Self::Io(_) => MdErrorCode::Io,
            Self::Core(_) => MdErrorCode::Core,
            Self::Blueprint(_) => MdErrorCode::Blueprint,
            Self::Foundation(_) => MdErrorCode::Foundation,
        }
    }
}

/// Result alias used throughout smithy-md.
pub type MdResult<T> = Result<T, MdError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_display_format() {
        assert_eq!(MdErrorCode::InvalidFrontmatter.to_string(), "E6000");
        assert_eq!(MdErrorCode::LosslessParse.to_string(), "E6008");
        assert_eq!(MdErrorCode::Foundation.to_string(), "E6007");
    }

    #[test]
    fn code_mapping_for_frontmatter() {
        let err = MdError::FrontmatterUnclosed;
        assert_eq!(err.code(), MdErrorCode::FrontmatterUnclosed);
    }

    #[test]
    fn unsupported_structure_variant_has_code() {
        let err = MdError::UnsupportedStructure { detail: "definition list".to_string() };
        assert_eq!(err.code(), MdErrorCode::UnsupportedStructure);
    }

    #[test]
    fn lossless_attribute_error_code_mapping() {
        let err = MdError::LosslessMissingAttribute { element: "img", attribute: "src" };
        assert_eq!(err.code(), MdErrorCode::LosslessMissingAttribute);
    }

    #[test]
    fn asset_contract_error_codes_and_display() {
        let at = crate::assets::RunLocator::new(3, 1);
        let mismatch =
            MdError::AssetPlanMismatch { occurrence: at, detail: "not provided".to_string() };
        assert_eq!(mismatch.code(), MdErrorCode::AssetPlanMismatch);
        assert_eq!(MdErrorCode::AssetPlanMismatch.to_string(), "E6017");
        assert!(mismatch.to_string().contains("paragraph 3 run 1"), "{mismatch}");

        let conflict =
            MdError::AssetIdentityConflict { occurrence: at, identity: "opaque:x".to_string() };
        assert_eq!(conflict.code(), MdErrorCode::AssetIdentityConflict);
        assert_eq!(MdErrorCode::AssetIdentityConflict.to_string(), "E6018");
    }

    #[test]
    fn file_too_large_error_code_and_display() {
        let err = MdError::FileTooLarge { size: 100_000_000, limit: 50_000_000 };
        assert_eq!(err.code(), MdErrorCode::FileTooLarge);
        assert!(err.to_string().contains("100000000"));
        assert!(err.to_string().contains("50000000"));
    }
}
