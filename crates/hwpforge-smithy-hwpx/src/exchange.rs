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

/// Preservation metadata required for byte-preserving section patching.
///
/// This payload is intentionally opaque to end users: `to-json --section`
/// produces it, and `patch` consumes it to update only the touched `<hp:t>`
/// payloads inside the original `sectionN.xml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SectionPreservation {
    /// Archive path of the source section entry (for example `Contents/section0.xml`).
    pub section_path: String,
    /// SHA-256 digest of the original section XML bytes in lowercase hex.
    pub section_sha256: String,
    /// Text slots, in semantic traversal order, that can be safely rewritten.
    pub text_slots: Vec<PreservedTextSlot>,
}

/// One patchable text slot inside a section export.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PreservedTextSlot {
    /// Stable semantic path (for debugging and validation).
    pub path: String,
    /// Original decoded text content for this slot.
    pub original_text: String,
    /// Whether the original `<hp:t>` contained inline HWPX child markup such
    /// as `<hp:tab/>` or `<hp:fwSpace/>`.
    #[serde(default)]
    pub has_inline_markup: bool,
    /// Raw XML locator used to apply the patch without regenerating the section.
    pub locator: TextLocator,
}

/// Raw XML locator for a patchable text slot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum TextLocator {
    /// Existing `<hp:t>` element inside the raw section XML.
    TextElement {
        /// Start byte offset of the full `<hp:t...>` element.
        element_start: usize,
        /// End byte offset of the full `<hp:t...>` element.
        element_end: usize,
        /// Start byte offset of the element's inner text content, if the
        /// element is not self-closing.
        content_start: Option<usize>,
        /// End byte offset of the element's inner text content, if the
        /// element is not self-closing.
        content_end: Option<usize>,
    },
    /// Empty `<hp:run>` placeholder that Core normalized into an empty text run.
    EmptyRun {
        /// Start byte offset of the `<hp:run...>` element.
        run_start: usize,
        /// End byte offset of the `<hp:run...>` element.
        run_end: usize,
    },
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
    /// Optional preservation metadata for byte-preserving section patching.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preservation: Option<SectionPreservation>,
}
