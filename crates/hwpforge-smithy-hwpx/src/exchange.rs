//! Shared exchange types for JSON round-trip editing.
//!
//! These types define the wire format used by both the CLI (`hwpforge to-json` / `patch`)
//! and the MCP server (`hwpforge_to_json` / `hwpforge_patch`). Having a single definition
//! ensures both bindings produce and consume identical JSON.

use serde::{Deserialize, Serialize};

use hwpforge_core::document::Document;
use hwpforge_core::section::Section;
use hwpforge_core::Draft;

use crate::HwpxStyleStore;

/// Full document export for JSON round-trip.
///
/// Produced by `to-json` (without `--section`) and consumed by `from-json`.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ExportedDocument {
    /// The Core document in Draft state.
    pub document: Document<Draft>,
    /// Optional style information for round-trip fidelity.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub styles: Option<HwpxStyleStore>,
}

/// Section-only export for targeted editing.
///
/// Produced by `to-json --section N` and consumed by `patch`.
#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ExportedSection {
    /// Which section index this was extracted from.
    pub section_index: usize,
    /// The section data.
    pub section: Section,
    /// Optional style information.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schemars", schemars(skip))]
    pub styles: Option<HwpxStyleStore>,
}
