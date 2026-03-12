//! MCP tool definitions for HwpForge.
//!
//! Each tool corresponds to a document lifecycle action:
//! - `convert`: Create (Markdown → HWPX)
//! - `inspect`: Read (HWPX structure summary)
//! - `to_json`: Read for Edit (HWPX → JSON)
//! - `from_json`: Create from JSON (JSON → HWPX)
//! - `patch`: Update (JSON → HWPX section replacement)
//! - `validate`: Verify (HWPX structure/integrity check)
//! - `restyle`: Update Style (apply different preset)
//! - `templates`: Discover (available style presets)
//! - `to_md`: Export (HWPX → Markdown)

pub mod convert;
pub mod from_json;
pub mod inspect;
pub mod patch;
pub mod restyle;
pub mod templates;
pub mod to_json;
pub mod to_md;
pub mod validate;
