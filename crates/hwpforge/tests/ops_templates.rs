//! `ops::style::templates` — the preset catalogue.
//!
//! The operation has no input and no failure mode, so what is worth
//! pinning is the *shape*: the key set the FFI hands to Python, and the
//! promise that every preset a caller can name is usable by `restyle`.
#![cfg(feature = "ops-hwpx")]

use hwpforge::ops::style::{restyle, templates, RestyleOptions};

fn keys(value: &serde_json::Value) -> Vec<String> {
    value.as_object().expect("an object").keys().cloned().collect()
}

#[test]
fn lists_the_four_built_in_presets() {
    let out = templates();

    let names: Vec<&str> = out.presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["default", "modern", "classic", "latest"], "declaration order is the API");
}

#[test]
fn meta_carries_exactly_the_presets_key() {
    let meta = templates().meta();

    let value = serde_json::to_value(&meta).expect("serialise");
    assert_eq!(keys(&value), ["presets"], "listing presets diagnoses nothing, so no warnings key");
}

#[test]
fn each_preset_serialises_with_the_four_documented_fields() {
    let value = serde_json::to_value(templates().meta()).expect("serialise");

    let presets = value["presets"].as_array().expect("an array");
    assert!(!presets.is_empty());
    for preset in presets {
        let mut fields = keys(preset);
        fields.sort();
        assert_eq!(fields, ["description", "font", "name", "page_size"], "{preset}");
    }
}

#[test]
fn meta_round_trips_through_json() {
    // `PresetInfo` gained `Deserialize` for this wrapper (smithy-hwpx
    // `presets.rs`), so the payload must not be write-only: the `.pyi` stub
    // and the pytest contract test both read the shape back.
    let meta = templates().meta();

    let json = serde_json::to_string(&meta).expect("serialise");
    let back: hwpforge::ops::style::TemplateList =
        serde_json::from_str(&json).expect("deserialise");

    assert_eq!(back.presets.len(), meta.presets.len());
    for (a, b) in back.presets.iter().zip(&meta.presets) {
        assert_eq!(
            (&a.name, &a.description, &a.font, &a.page_size),
            (&b.name, &b.description, &b.font, &b.page_size)
        );
    }
}

#[test]
fn every_listed_preset_is_one_restyle_accepts() {
    // The catalogue is the answer to "what may I pass to restyle?" — a name
    // listed here that restyle rejects would make the catalogue a lie.
    // Bad bytes are enough: the preset is looked up before anything is
    // decoded, so a wrong name reports `PRESET_NOT_FOUND` rather than
    // `DECODE_FAILED`.
    for preset in templates().presets {
        let err = restyle(b"not an hwpx", &RestyleOptions::default().with_preset(&preset.name))
            .expect_err("bad bytes must still fail");

        assert_eq!(
            err.code().as_str(),
            "DECODE_FAILED",
            "preset `{}` is listed but restyle refused the name: {err}",
            preset.name
        );
    }
}
