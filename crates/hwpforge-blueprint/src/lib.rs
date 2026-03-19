//! HwpForge Blueprint: YAML-based style template system.
//!
//! Blueprint is the **design pattern** layer in the Forge metaphor.
//! It defines style templates (fonts, sizes, colors, spacing, tab stops) in
//! human-readable YAML that can be applied to Core documents.
//!
//! # Architecture
//!
//! ```text
//! foundation (HwpUnit, Color, Index<T>)
//!     |
//!     v
//! core (Document, Section, Paragraph, Run)
//!     |
//!     v
//! blueprint (THIS CRATE: Template, StyleRegistry, CharShape, ParaShape, TabDef)
//!     |
//!     v
//! smithy-* (HWPX, HWP5, Markdown codecs)
//! ```
//!
//! Core contains document **structure** with style **references** (indices).
//! Blueprint contains style **definitions** that those indices resolve to.
//! This separation mirrors HTML (structure) + CSS (style).
//!
//! # Quick Start
//!
//! ```rust
//! use hwpforge_blueprint::template::Template;
//! use hwpforge_blueprint::registry::StyleRegistry;
//! use hwpforge_blueprint::builtins::builtin_default;
//!
//! // Load a built-in template
//! let template = builtin_default().unwrap();
//! assert_eq!(template.meta.name, "default");
//!
//! // Convert to indexed registry for document rendering
//! let registry = StyleRegistry::from_template(&template).unwrap();
//! let body = registry.get_style("body").unwrap();
//! let char_shape = registry.char_shape(body.char_shape_id).unwrap();
//! assert_eq!(char_shape.font, "한컴바탕");
//! ```
//!
//! # Workflow
//!
//! ```text
//! YAML template file
//!   -> Template::from_yaml()
//!     -> Inheritance resolution (DFS merge)
//!       -> StyleRegistry::from_template()
//!         -> Indexed CharShape/ParaShape/Tab collections
//! ```

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![deny(clippy::all)]

pub mod border_fill;
pub mod builtins;
pub mod error;
pub mod inheritance;
pub mod registry;
pub mod schema;
pub mod serde_helpers;
pub mod style;
pub mod template;

// Re-export key types for convenience
pub use border_fill::{Border, BorderFill, BorderSide, Fill, PartialBorderFill};
