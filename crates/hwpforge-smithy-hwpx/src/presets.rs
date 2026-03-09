//! Built-in style presets for HWPX document generation.
//!
//! Shared between CLI and MCP bindings so that preset data stays in sync.

use serde::Serialize;

/// Information about a single style preset.
#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    /// Preset name (e.g., `"default"`).
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Base font name.
    pub font: String,
    /// Page size (e.g., `"A4"`).
    pub page_size: String,
}

/// Returns the list of built-in presets.
///
/// Currently only `"default"` is available. Additional presets
/// (`government`, `report`, `official`) are planned.
pub fn builtin_presets() -> Vec<PresetInfo> {
    vec![PresetInfo {
        name: "default".to_string(),
        description: "한컴 Modern 기본 스타일 (함초롬돋움 10pt, A4)".to_string(),
        font: "함초롬돋움".to_string(),
        page_size: "A4".to_string(),
    }]
}
