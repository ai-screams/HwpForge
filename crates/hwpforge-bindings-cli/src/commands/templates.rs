//! Manage style presets.

use serde::Serialize;

use crate::error::CliError;

#[derive(Serialize)]
struct PresetInfo {
    name: String,
    description: String,
    font: String,
    page_size: String,
}

fn builtin_presets() -> Vec<PresetInfo> {
    vec![PresetInfo {
        name: "default".to_string(),
        description: "한컴 Modern 기본 스타일 (함초롬돋움 10pt, A4)".to_string(),
        font: "함초롬돋움".to_string(),
        page_size: "A4".to_string(),
    }]
}

/// List all available presets.
pub fn run_list(json_mode: bool) {
    let presets = builtin_presets();
    if json_mode {
        let result = serde_json::json!({
            "status": "ok",
            "presets": presets,
        });
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!("Available presets:");
        for p in &presets {
            println!("  {} — {}", p.name, p.description);
        }
    }
}

/// Show details of a specific preset.
pub fn run_show(name: &str, json_mode: bool) {
    let presets = builtin_presets();
    match presets.into_iter().find(|p| p.name == name) {
        Some(p) => {
            if json_mode {
                let result = serde_json::json!({
                    "status": "ok",
                    "preset": p,
                });
                println!("{}", serde_json::to_string(&result).unwrap());
            } else {
                println!("Preset: {}", p.name);
                println!("  Description: {}", p.description);
                println!("  Font: {}", p.font);
                println!("  Page: {}", p.page_size);
            }
        }
        None => {
            CliError::new("PRESET_NOT_FOUND", format!("Preset '{name}' not found"))
                .with_hint("Run 'hwpforge templates list' to see available presets")
                .exit(json_mode, 1);
        }
    }
}
