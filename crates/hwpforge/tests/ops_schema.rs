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

/// Group JSON as 0.16.10 wrote it (`to-json` of Hancom's F1 g4, a nested
/// group, converted from HWP5 by that release; its children are plain shape
/// controls) validates against the schema this release publishes and reads
/// into the same groups, every child a shape.
///
/// 이것을 실패시키는 것: 스키마에서 `GroupChild` 의 Shape 갈래(`$ref Control`)를 빼는
/// 것(검증 실패) / `GroupChild` 를 외부 태그 derive 로 바꾸는 것(옛 자식이 읽히지 않음).
#[test]
fn group_payload_from_0_16_10_still_validates_and_reads() {
    use hwpforge::core::control::{Control, GroupChild};
    use hwpforge::core::run::RunContent;

    let old = std::fs::read_to_string(format!("{DATA}v0_16_10_group_document.json")).unwrap();
    let mut instance: serde_json::Value = serde_json::from_str(&old).unwrap();
    hwpforge::hwpx::exchange::read_exchange_v2(&mut instance).unwrap();
    // Canonical v2 serialization also applies older non-measure aliases (shadow_type).
    let instance = serde_json::to_value(
        serde_json::from_value::<hwpforge::hwpx::ExportedDocument>(instance).unwrap(),
    )
    .unwrap();
    let snapshot: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(snapshot_path(SchemaKind::ExportedDocument)).unwrap(),
    )
    .unwrap();
    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    compiler.add_resource("urn:hwpforge:exported-document", snapshot).unwrap();
    let index = compiler.compile("urn:hwpforge:exported-document", &mut schemas).unwrap();
    if let Err(e) = schemas.validate(&instance, index) {
        panic!("0.16.10 group JSON no longer validates: {e:#}");
    }

    // The shared migration changes only paragraph measures, preserving the legacy group payload.
    let doc: hwpforge::core::Document<hwpforge::core::Draft> =
        serde_json::from_value(instance["document"].clone()).expect("group document reads");
    fn shapes(children: &[GroupChild]) -> usize {
        children
            .iter()
            .map(|c| match c {
                GroupChild::Shape(Control::Group { children, .. }) => 1 + shapes(children),
                GroupChild::Shape(_) => 1,
                _ => panic!("not a shape: {c:?}"),
            })
            .sum()
    }
    let counted: usize = doc.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| p.runs.iter())
        .filter_map(|r| match &r.content {
            RunContent::Control(c) => match c.as_ref() {
                Control::Group { children, .. } => Some(shapes(children)),
                _ => None,
            },
            _ => None,
        })
        .sum();
    // Count the children in the raw JSON the same way.
    fn raw(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Object(m) => match m.get("Group") {
                Some(g) => g["children"].as_array().unwrap().iter().map(|c| 1 + raw_inner(c)).sum(),
                None => m.values().map(raw).sum(),
            },
            serde_json::Value::Array(a) => a.iter().map(raw).sum(),
            _ => 0,
        }
    }
    fn raw_inner(child: &serde_json::Value) -> usize {
        child.get("Group").map_or(0, |_| raw(child))
    }
    assert!(counted > 0);
    assert_eq!(counted, raw(&instance["document"]));
}

/// A document exported by 0.16.10 still validates against the published
/// `exported-document` schema and still builds a package.
///
/// `tests/data/master_pages_v0_16_10.json` is the 0.16.10 release binary's
/// output for a document with even and odd master pages, exported without styles.
///
/// Its master pages carry only `apply_page_type` and `paragraphs`; every
/// field added since must stay optional in the schema.
///
/// 이것을 실패시키는 것: 새 `MasterPage` 필드 하나의 `#[serde(default)]` 를
/// 지우기 — schemars 가 그 필드를 `required` 로 올린다.
#[test]
fn a_0_16_10_master_page_export_validates_against_the_current_schema() {
    use hwpforge::ops::exchange::{from_json, FromJsonOptions};

    let path = format!("{DATA}master_pages_v0_16_10.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut old: serde_json::Value = serde_json::from_str(&text).expect("old export is JSON");
    hwpforge::hwpx::exchange::read_exchange_v2(&mut old).unwrap();
    let masters = &old["document"]["sections"][0]["master_pages"];
    assert_eq!(masters[0]["apply_page_type"], "Even", "the fixture is the even/odd one");
    assert_eq!(masters[0].as_object().map(|o| o.len()), Some(2), "only the 0.16.10 fields");

    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    compiler
        .add_resource("urn:exported-document", produced(SchemaKind::ExportedDocument))
        .expect("schema resource");
    let id = compiler.compile("urn:exported-document", &mut schemas).expect("schema compiles");
    if let Err(e) = schemas.validate(&old, id) {
        panic!("0.16.10 export no longer validates: {e:#}");
    }

    let built = from_json(&text, &FromJsonOptions::default()).expect("old export still builds");
    assert_eq!(built.sections, 1);
}
