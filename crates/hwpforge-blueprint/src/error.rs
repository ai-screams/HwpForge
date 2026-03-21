//! Error types for the Blueprint crate.
//!
//! Blueprint errors occupy the **3000-3999** range in the HwpForge
//! error code scheme.
//!
//! # Error Code Ranges
//!
//! | Range     | Crate       |
//! |-----------|-------------|
//! | 1000-1999 | Foundation  |
//! | 2000-2999 | Core        |
//! | 3000-3999 | Blueprint   |
//! | 4000-4999 | Smithy-HWPX |

use std::fmt;

use hwpforge_foundation::{
    error::{ErrorCode, ErrorCodeExt, FoundationError},
    HeadingType,
};

/// Numeric error codes for Blueprint (3000-3999).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum BlueprintErrorCode {
    /// YAML syntax or structure error.
    YamlParse = 3000,
    /// Invalid dimension string (e.g. "16px" instead of "16pt").
    InvalidDimension = 3001,
    /// Circular template inheritance detected.
    CircularInheritance = 3002,
    /// Referenced parent template not found.
    TemplateNotFound = 3003,
    /// Inheritance chain exceeds depth limit.
    InheritanceDepthExceeded = 3004,
    /// Style map is empty (no styles defined).
    EmptyStyleMap = 3005,
    /// Style resolution failed (missing required fields).
    StyleResolution = 3006,
    /// Duplicate style name in the same scope.
    DuplicateStyleName = 3007,
    /// Invalid percentage string.
    InvalidPercentage = 3008,
    /// Invalid color string.
    InvalidColor = 3009,
    /// Markdown mapping references unknown style.
    InvalidMappingReference = 3010,
    /// Invalid style name.
    InvalidStyleName = 3011,
    /// Paragraph references an unknown or reserved tab definition.
    InvalidTabReference = 3012,
    /// Duplicate tab definition id.
    DuplicateTabDefinition = 3013,
    /// Tab definition contains invalid stop ordering or duplicate positions.
    InvalidTabDefinition = 3014,
    /// Duplicate numbering definition id.
    DuplicateNumberingDefinition = 3015,
    /// Duplicate bullet definition id.
    DuplicateBulletDefinition = 3016,
    /// Paragraph list level is outside the supported range.
    InvalidListLevel = 3017,
    /// Paragraph references an unknown numbering/bullet definition.
    InvalidListReference = 3018,
    /// Paragraph mixes legacy heading_type with explicit list semantics.
    ConflictingListSpecification = 3019,
    /// Legacy heading_type cannot be migrated safely.
    UnsupportedLegacyHeadingType = 3020,
    /// Style references a non-checkable bullet as a checkable list.
    InvalidCheckableBulletDefinition = 3021,
    /// Bullet definition mixes checkbox fields incoherently.
    InvalidBulletDefinition = 3022,
    /// Style references a checkable bullet as a plain bullet list.
    InvalidPlainBulletDefinition = 3023,
}

impl fmt::Display for BlueprintErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", *self as u32)
    }
}

/// The primary error type for the Blueprint crate.
#[derive(Debug, Clone, thiserror::Error)]
#[non_exhaustive]
pub enum BlueprintError {
    /// YAML parsing or structure error.
    #[error("YAML parse error: {message}")]
    YamlParse {
        /// Description of the parse failure.
        message: String,
    },

    /// Invalid dimension string.
    #[error("invalid dimension '{value}': expected format like '16pt', '20mm', or '1in'")]
    InvalidDimension {
        /// The invalid input.
        value: String,
    },

    /// Invalid percentage string.
    #[error("invalid percentage '{value}': expected format like '160%'")]
    InvalidPercentage {
        /// The invalid input.
        value: String,
    },

    /// Invalid color string.
    #[error("invalid color '{value}': expected '#RRGGBB' format")]
    InvalidColor {
        /// The invalid input.
        value: String,
    },

    /// Circular template inheritance detected.
    #[error("circular template inheritance: {}", chain.join(" -> "))]
    CircularInheritance {
        /// The full chain showing the cycle.
        chain: Vec<String>,
    },

    /// Referenced parent template not found.
    #[error("template not found: '{name}'")]
    TemplateNotFound {
        /// The missing template name.
        name: String,
    },

    /// Inheritance chain exceeds depth limit.
    #[error("inheritance depth {depth} exceeds maximum {max}")]
    InheritanceDepthExceeded {
        /// Actual depth reached.
        depth: usize,
        /// Configured maximum.
        max: usize,
    },

    /// Style map is empty.
    #[error("template has no styles defined")]
    EmptyStyleMap,

    /// A style could not be fully resolved (missing required fields).
    #[error("cannot resolve style '{style_name}': missing required field '{field}'")]
    StyleResolution {
        /// Name of the unresolvable style.
        style_name: String,
        /// The missing field.
        field: String,
    },

    /// Duplicate style name.
    #[error("duplicate style name '{name}'")]
    DuplicateStyleName {
        /// The duplicated name.
        name: String,
    },

    /// Markdown mapping references a non-existent style.
    #[error("markdown mapping '{mapping_field}' references unknown style '{style_name}'")]
    InvalidMappingReference {
        /// The markdown element field (e.g. "heading1").
        mapping_field: String,
        /// The style name that was referenced but not found.
        style_name: String,
    },

    /// Invalid style name.
    #[error("invalid style name '{name}': {reason}")]
    InvalidStyleName {
        /// The invalid name.
        name: String,
        /// Why it's invalid.
        reason: String,
    },

    /// Paragraph references an unknown or reserved tab definition.
    #[error("style '{style_name}' references invalid tab definition {tab_id}: {reason}")]
    InvalidTabReference {
        /// Style name that contains the invalid tab reference.
        style_name: String,
        /// Referenced tab definition id.
        tab_id: u32,
        /// Why the reference is invalid.
        reason: String,
    },

    /// Multiple tab definitions share the same id.
    #[error("duplicate tab definition id {id}")]
    DuplicateTabDefinition {
        /// The duplicated id.
        id: u32,
    },

    /// A tab definition contains invalid stop ordering or duplicate positions.
    #[error("tab definition {id} is invalid: {reason}")]
    InvalidTabDefinition {
        /// The invalid tab definition id.
        id: u32,
        /// Why it is invalid.
        reason: String,
    },

    /// Multiple numbering definitions share the same id.
    #[error("duplicate numbering definition id {id}")]
    DuplicateNumberingDefinition {
        /// The duplicated numbering definition id.
        id: u32,
    },

    /// Multiple bullet definitions share the same id.
    #[error("duplicate bullet definition id {id}")]
    DuplicateBulletDefinition {
        /// The duplicated bullet definition id.
        id: u32,
    },

    /// A paragraph list level is outside the supported shared IR range.
    #[error("style '{style_name}' uses invalid list level {level}: expected 0..={max}")]
    InvalidListLevel {
        /// Style name that contains the invalid level.
        style_name: String,
        /// Invalid level value.
        level: u8,
        /// Highest supported level.
        max: u8,
    },

    /// A paragraph references a numbering/bullet definition that does not exist.
    #[error("style '{style_name}' references unknown {kind} definition {id}")]
    InvalidListReference {
        /// Style name that contains the bad reference.
        style_name: String,
        /// Referenced list kind (`numbering` or `bullet`).
        kind: String,
        /// Referenced definition id.
        id: u32,
    },

    /// A paragraph tries to use both legacy and explicit list syntax.
    #[error("style '{style_name}' mixes legacy heading_type with explicit para_shape.list")]
    ConflictingListSpecification {
        /// Style name that contains the conflict.
        style_name: String,
    },

    /// A legacy heading type was supplied without enough information to build
    /// a shared list reference.
    #[error(
        "style '{style_name}' uses unsupported legacy heading_type '{heading_type:?}'; use para_shape.list instead"
    )]
    UnsupportedLegacyHeadingType {
        /// Style name that contains the legacy field.
        style_name: String,
        /// The unsupported legacy heading type.
        heading_type: HeadingType,
    },

    /// A style references a bullet definition that cannot represent checkbox state.
    #[error("style '{style_name}' references non-checkable bullet definition {bullet_id}")]
    InvalidCheckableBulletDefinition {
        /// Style name that contains the invalid checkable list reference.
        style_name: String,
        /// Referenced bullet definition id.
        bullet_id: u32,
    },

    /// A style references a checkable bullet definition through plain bullet semantics.
    #[error(
        "style '{style_name}' references checkable bullet definition {bullet_id} as a plain bullet"
    )]
    InvalidPlainBulletDefinition {
        /// Style name that contains the invalid plain bullet reference.
        style_name: String,
        /// Referenced bullet definition id.
        bullet_id: u32,
    },

    /// A bullet definition is internally inconsistent for authoring.
    #[error("bullet definition {id} is invalid: {reason}")]
    InvalidBulletDefinition {
        /// The invalid bullet definition id.
        id: u32,
        /// Why it is invalid.
        reason: String,
    },

    /// Propagated Foundation error.
    #[error(transparent)]
    Foundation(#[from] FoundationError),
}

impl ErrorCodeExt for BlueprintError {
    fn code(&self) -> ErrorCode {
        // Blueprint uses its own BlueprintErrorCode internally,
        // but maps to Foundation's ErrorCode for cross-crate compatibility.
        // Since Foundation's ErrorCode doesn't cover Blueprint ranges,
        // we map to the closest generic code.
        match self {
            Self::Foundation(e) => e.code(),
            Self::InvalidDimension { .. } | Self::InvalidPercentage { .. } => {
                ErrorCode::InvalidField
            }
            Self::InvalidColor { .. } => ErrorCode::InvalidColor,
            _ => ErrorCode::InvalidField,
        }
    }
}

impl BlueprintError {
    /// Returns the Blueprint-specific error code.
    pub fn blueprint_code(&self) -> BlueprintErrorCode {
        match self {
            Self::YamlParse { .. } => BlueprintErrorCode::YamlParse,
            Self::InvalidDimension { .. } => BlueprintErrorCode::InvalidDimension,
            Self::InvalidPercentage { .. } => BlueprintErrorCode::InvalidPercentage,
            Self::InvalidColor { .. } => BlueprintErrorCode::InvalidColor,
            Self::CircularInheritance { .. } => BlueprintErrorCode::CircularInheritance,
            Self::TemplateNotFound { .. } => BlueprintErrorCode::TemplateNotFound,
            Self::InheritanceDepthExceeded { .. } => BlueprintErrorCode::InheritanceDepthExceeded,
            Self::EmptyStyleMap => BlueprintErrorCode::EmptyStyleMap,
            Self::StyleResolution { .. } => BlueprintErrorCode::StyleResolution,
            Self::DuplicateStyleName { .. } => BlueprintErrorCode::DuplicateStyleName,
            Self::InvalidMappingReference { .. } => BlueprintErrorCode::InvalidMappingReference,
            Self::InvalidStyleName { .. } => BlueprintErrorCode::InvalidStyleName,
            Self::InvalidTabReference { .. } => BlueprintErrorCode::InvalidTabReference,
            Self::DuplicateTabDefinition { .. } => BlueprintErrorCode::DuplicateTabDefinition,
            Self::InvalidTabDefinition { .. } => BlueprintErrorCode::InvalidTabDefinition,
            Self::DuplicateNumberingDefinition { .. } => {
                BlueprintErrorCode::DuplicateNumberingDefinition
            }
            Self::DuplicateBulletDefinition { .. } => BlueprintErrorCode::DuplicateBulletDefinition,
            Self::InvalidListLevel { .. } => BlueprintErrorCode::InvalidListLevel,
            Self::InvalidListReference { .. } => BlueprintErrorCode::InvalidListReference,
            Self::ConflictingListSpecification { .. } => {
                BlueprintErrorCode::ConflictingListSpecification
            }
            Self::UnsupportedLegacyHeadingType { .. } => {
                BlueprintErrorCode::UnsupportedLegacyHeadingType
            }
            Self::InvalidCheckableBulletDefinition { .. } => {
                BlueprintErrorCode::InvalidCheckableBulletDefinition
            }
            Self::InvalidBulletDefinition { .. } => BlueprintErrorCode::InvalidBulletDefinition,
            Self::InvalidPlainBulletDefinition { .. } => {
                BlueprintErrorCode::InvalidPlainBulletDefinition
            }
            Self::Foundation(_) => BlueprintErrorCode::YamlParse, // fallback
        }
    }
}

/// Convenience type alias for Blueprint operations.
pub type BlueprintResult<T> = Result<T, BlueprintError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_display_format() {
        assert_eq!(BlueprintErrorCode::YamlParse.to_string(), "E3000");
        assert_eq!(BlueprintErrorCode::InvalidDimension.to_string(), "E3001");
        assert_eq!(BlueprintErrorCode::CircularInheritance.to_string(), "E3002");
        assert_eq!(BlueprintErrorCode::InvalidColor.to_string(), "E3009");
    }

    #[test]
    fn error_code_range_is_3000() {
        assert_eq!(BlueprintErrorCode::YamlParse as u32, 3000);
        assert_eq!(BlueprintErrorCode::InvalidColor as u32, 3009);
    }

    #[test]
    fn yaml_parse_error_message() {
        let err = BlueprintError::YamlParse { message: "unexpected key 'foo'".into() };
        assert_eq!(err.to_string(), "YAML parse error: unexpected key 'foo'");
        assert_eq!(err.blueprint_code(), BlueprintErrorCode::YamlParse);
    }

    #[test]
    fn invalid_dimension_error_message() {
        let err = BlueprintError::InvalidDimension { value: "16px".into() };
        assert!(err.to_string().contains("16px"));
        assert!(err.to_string().contains("16pt"));
    }

    #[test]
    fn circular_inheritance_shows_chain() {
        let err =
            BlueprintError::CircularInheritance { chain: vec!["a".into(), "b".into(), "a".into()] };
        assert_eq!(err.to_string(), "circular template inheritance: a -> b -> a");
    }

    #[test]
    fn template_not_found_message() {
        let err = BlueprintError::TemplateNotFound { name: "missing_template".into() };
        assert!(err.to_string().contains("missing_template"));
    }

    #[test]
    fn inheritance_depth_exceeded_message() {
        let err = BlueprintError::InheritanceDepthExceeded { depth: 15, max: 10 };
        assert!(err.to_string().contains("15"));
        assert!(err.to_string().contains("10"));
    }

    #[test]
    fn style_resolution_error_message() {
        let err =
            BlueprintError::StyleResolution { style_name: "heading1".into(), field: "font".into() };
        assert!(err.to_string().contains("heading1"));
        assert!(err.to_string().contains("font"));
    }

    #[test]
    fn foundation_error_propagation() {
        let foundation_err = FoundationError::EmptyIdentifier { item: "FontId".into() };
        let err: BlueprintError = foundation_err.into();
        assert!(matches!(err, BlueprintError::Foundation(_)));
        assert!(err.to_string().contains("FontId"));
    }

    #[test]
    fn error_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BlueprintError>();
    }

    #[test]
    fn error_implements_std_error() {
        fn assert_error<T: std::error::Error>() {}
        assert_error::<BlueprintError>();
    }

    #[test]
    fn blueprint_code_mapping() {
        let cases: Vec<(BlueprintError, BlueprintErrorCode)> = vec![
            (BlueprintError::YamlParse { message: String::new() }, BlueprintErrorCode::YamlParse),
            (
                BlueprintError::InvalidDimension { value: String::new() },
                BlueprintErrorCode::InvalidDimension,
            ),
            (
                BlueprintError::InvalidPercentage { value: String::new() },
                BlueprintErrorCode::InvalidPercentage,
            ),
            (
                BlueprintError::InvalidColor { value: String::new() },
                BlueprintErrorCode::InvalidColor,
            ),
            (
                BlueprintError::CircularInheritance { chain: vec![] },
                BlueprintErrorCode::CircularInheritance,
            ),
            (
                BlueprintError::TemplateNotFound { name: String::new() },
                BlueprintErrorCode::TemplateNotFound,
            ),
            (
                BlueprintError::InheritanceDepthExceeded { depth: 0, max: 0 },
                BlueprintErrorCode::InheritanceDepthExceeded,
            ),
            (BlueprintError::EmptyStyleMap, BlueprintErrorCode::EmptyStyleMap),
            (
                BlueprintError::StyleResolution { style_name: String::new(), field: String::new() },
                BlueprintErrorCode::StyleResolution,
            ),
            (
                BlueprintError::DuplicateStyleName { name: String::new() },
                BlueprintErrorCode::DuplicateStyleName,
            ),
            (
                BlueprintError::InvalidMappingReference {
                    mapping_field: String::new(),
                    style_name: String::new(),
                },
                BlueprintErrorCode::InvalidMappingReference,
            ),
            (
                BlueprintError::InvalidStyleName { name: String::new(), reason: String::new() },
                BlueprintErrorCode::InvalidStyleName,
            ),
            (
                BlueprintError::InvalidBulletDefinition { id: 0, reason: String::new() },
                BlueprintErrorCode::InvalidBulletDefinition,
            ),
            (
                BlueprintError::InvalidPlainBulletDefinition {
                    style_name: String::new(),
                    bullet_id: 0,
                },
                BlueprintErrorCode::InvalidPlainBulletDefinition,
            ),
        ];

        for (err, expected_code) in cases {
            assert_eq!(err.blueprint_code(), expected_code);
        }
    }

    #[test]
    fn error_code_ext_for_foundation_passthrough() {
        let err = BlueprintError::Foundation(FoundationError::InvalidHwpUnit {
            value: 999_999_999,
            min: -100_000_000,
            max: 100_000_000,
        });
        assert_eq!(err.code(), ErrorCode::InvalidHwpUnit);
    }
}
