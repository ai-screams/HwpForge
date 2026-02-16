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
    /// I/O failure.
    Io = 6004,
    /// Propagated Core error.
    Core = 6005,
    /// Propagated Blueprint error.
    Blueprint = 6006,
    /// Propagated Foundation error.
    Foundation = 6007,
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
}
