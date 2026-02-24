//! Internal macros for reducing boilerplate.
//!
//! These macros are `pub(crate)` -- they generate public types but
//! are not themselves part of the public API.

/// Generates a newtype wrapper around `String` with validation, accessors,
/// and standard trait implementations.
///
/// Each generated type:
/// - Validates that the input is non-empty on construction
/// - Provides `as_str() -> &str`
/// - Derives: `Debug`, `Clone`, `PartialEq`, `Eq`, `Hash`, `Serialize`,
///   `Deserialize`
/// - Implements `Display`, `JsonSchema`
///
/// # Phase 1 Migration Note
///
/// The internal representation will migrate from `String` to an interned
/// type (`lasso` or `string_cache::Atom`) for O(1) comparison and memory
/// deduplication. The public API (`new`, `as_str`) remains identical.
macro_rules! string_newtype {
    (
        $(#[$meta:meta])*
        $name:ident, $label:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
        #[repr(transparent)]
        pub struct $name(String);

        impl $name {
            /// Creates a new identifier, validating it is non-empty.
            ///
            /// # Errors
            ///
            /// Returns [`crate::FoundationError::EmptyIdentifier`] when `value`
            /// is empty.
            pub fn new(value: impl Into<String>) -> $crate::FoundationResult<Self> {
                let s = value.into();
                if s.is_empty() {
                    return Err($crate::FoundationError::EmptyIdentifier {
                        item: $label.to_string(),
                    });
                }
                Ok(Self(s))
            }

            /// Returns the identifier as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                stringify!($name).into()
            }

            fn json_schema(
                gen: &mut schemars::SchemaGenerator,
            ) -> schemars::Schema {
                gen.subschema_for::<String>()
            }
        }
    };
}

pub(crate) use string_newtype;
