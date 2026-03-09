//! `hwpforge_templates` — Style preset discovery tool.

use serde::Serialize;

use crate::output::ToolErrorInfo;

/// Information about a single preset.
#[derive(Debug, Serialize)]
pub struct PresetInfo {
    /// Preset name (e.g., "default", "government").
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Base font name.
    pub font: String,
    /// Page size (e.g., "A4").
    pub page_size: String,
}

/// Output data from the templates tool.
#[derive(Debug, Serialize)]
pub struct TemplatesData {
    /// List of available presets.
    pub templates: Vec<PresetInfo>,
}

/// List built-in presets.
fn builtin_presets() -> Vec<PresetInfo> {
    vec![PresetInfo {
        name: "default".to_string(),
        description: "한컴 Modern 기본 스타일 (함초롬돋움 10pt, A4)".to_string(),
        font: "함초롬돋움".to_string(),
        page_size: "A4".to_string(),
    }]
}

/// Get available templates, optionally filtered by name.
pub fn run_templates(name: Option<&str>) -> Result<TemplatesData, ToolErrorInfo> {
    let presets = builtin_presets();

    if let Some(name) = name {
        let filtered: Vec<PresetInfo> = presets.into_iter().filter(|p| p.name == name).collect();
        if filtered.is_empty() {
            return Err(ToolErrorInfo::new(
                "PRESET_NOT_FOUND",
                format!("Preset '{name}' not found"),
                "Available presets: default. Use hwpforge_templates without a name to list all.",
            ));
        }
        Ok(TemplatesData { templates: filtered })
    } else {
        Ok(TemplatesData { templates: presets })
    }
}
