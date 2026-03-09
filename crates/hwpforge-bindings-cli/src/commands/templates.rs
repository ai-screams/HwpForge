//! Manage style presets.

use hwpforge_smithy_hwpx::presets::builtin_presets;

use crate::error::CliError;

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
