//! `ops::schema::schema` — the published exchange schemas.
//!
//! # Where the snapshots come from
//!
//! `tests/data/schema_*.json` were produced by the **CLI binary**, which is
//! what publishes these schemas today:
//!
//! ```text
//! cargo build -p hwpforge-bindings-cli --bin hwpforge
//! ./target/debug/hwpforge schema document          > crates/hwpforge/tests/data/schema_document.json
//! ./target/debug/hwpforge schema exported-document > crates/hwpforge/tests/data/schema_exported_document.json
//! ./target/debug/hwpforge schema exported-section  > crates/hwpforge/tests/data/schema_exported_section.json
//! ```
//!
//! That provenance is the point of the test: it proves the operation layer
//! publishes the same schema the CLI does, injected `addr` property and
//! all. `UPDATE_SCHEMA_SNAPSHOT=1` rewrites the files from *this*
//! implementation, which is the right move only after an intentional schema
//! change has been confirmed against the CLI with the commands above.
//!
//! # Why not `UPDATE_INVENTORY`
//!
//! The two inventory snapshots (`ops_codes.rs`, `ops_error_inventory.rs`)
//! share that variable, and regenerating them runs **every** test binary in
//! the crate. These three files are different in kind: their source of truth
//! is an external binary, not a table this workspace owns. Sharing the
//! variable would mean an unrelated inventory regeneration silently rewrites
//! them from the operation layer — and it would fail invisibly, because the
//! only time this test earns its keep is when ops has drifted from the CLI,
//! which is exactly when the accidental rewrite would erase the evidence.
#![cfg(all(feature = "ops-hwpx", feature = "schemars"))]

use hwpforge::ops::schema::{schema, SchemaKind, SchemaOptions};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/");

fn snapshot_path(kind: SchemaKind) -> String {
    format!("{DATA}schema_{}.json", kind.as_str().replace('-', "_"))
}

fn produced(kind: SchemaKind) -> serde_json::Value {
    schema(&SchemaOptions::default().with_kind(kind))
        .unwrap_or_else(|e| panic!("{}: {e}", kind.as_str()))
        .schema
}

const ALL: [SchemaKind; 3] =
    [SchemaKind::Document, SchemaKind::ExportedDocument, SchemaKind::ExportedSection];

#[test]
fn each_schema_matches_the_snapshot_taken_from_the_cli() {
    for kind in ALL {
        let path = snapshot_path(kind);
        let produced = produced(kind);

        if std::env::var_os("UPDATE_SCHEMA_SNAPSHOT").is_some() {
            let pretty = serde_json::to_string_pretty(&produced).expect("render");
            std::fs::write(&path, pretty + "\n").expect("write snapshot");
            continue;
        }

        let tracked: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}")),
        )
        .unwrap_or_else(|e| panic!("{path} is not JSON: {e}"));

        // Compared as values, not as text: `schemars` keeps insertion order
        // in an `IndexMap`, so a textual diff would flake on key order while
        // the schema itself is unchanged.
        assert_eq!(
            produced,
            tracked,
            "`{}` drifted from what the CLI publishes — rerun the commands in this file's \
             header, and only then UPDATE_SCHEMA_SNAPSHOT=1",
            kind.as_str()
        );
    }
}

#[test]
fn every_schema_documents_the_injected_cell_address() {
    // `to-json` adds `addr` after serde has run, so a consumer validating
    // real output against the derived schema alone would reject it.
    for kind in ALL {
        let addr = produced(kind)["$defs"]["TableCell"]["properties"]["addr"].clone();

        assert!(addr.is_object(), "{}: no injected addr", kind.as_str());
        assert_eq!(addr["type"], "object");
        assert_eq!(addr["required"], serde_json::json!(["row", "col"]));
        assert_eq!(addr["properties"]["row"]["format"], "uint32");
    }
}

#[test]
fn document_is_the_kind_you_get_without_asking() {
    assert_eq!(SchemaOptions::default().kind, SchemaKind::Document);
    assert_eq!(produced(SchemaKind::Document), schema(&SchemaOptions::default()).unwrap().schema);
}

#[test]
fn the_three_kinds_are_three_different_schemas() {
    let document = produced(SchemaKind::Document);
    let exported = produced(SchemaKind::ExportedDocument);
    let section = produced(SchemaKind::ExportedSection);

    assert_ne!(document, exported);
    assert_ne!(exported, section);
    assert_ne!(document, section);
}

#[test]
fn each_schema_declares_its_dialect_and_root_type() {
    for kind in ALL {
        let value = produced(kind);

        assert_eq!(
            value["$schema"],
            "https://json-schema.org/draft/2020-12/schema",
            "{}",
            kind.as_str()
        );
        assert_eq!(value["type"], "object", "{}", kind.as_str());
    }
}

#[test]
fn an_unknown_kind_name_is_rejected_with_the_valid_ones_listed() {
    let err = SchemaOptions::default().with_kind_name("exported_section").expect_err("must reject");

    assert_eq!(err.code().as_str(), "INVALID_INPUT", "{err}");
    assert!(err.to_string().contains("exported-section"), "{err}");
}
