//! MCP tool definitions for HwpForge.
//!
//! Each tool corresponds to a document lifecycle action:
//! - `convert`: Create (Markdown → HWPX)
//! - `inspect`: Read (HWPX structure summary)
//! - `to_json`: Read for Edit (HWPX → JSON)
//! - `patch`: Update (JSON → HWPX section replacement)
//! - `templates`: Discover (available style presets)
