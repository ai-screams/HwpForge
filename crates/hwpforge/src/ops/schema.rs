//! JSON schema export: `schema`.
//!
//! The schemas published here describe the JSON that `to_json`,
//! `export_section`, `from_json` and `patch` exchange. They are the derived
//! `schemars` output **plus one hand-written property** — see
//! [`schema`] for why.
//!
//! # Feature
//!
//! This module needs the `schemars` feature, because the schemas are
//! derived from the same `JsonSchema` implementations the exchange DTOs
//! carry. It is not gated on anything else: the Python bindings enable
//! `schemars` unconditionally so that `schema()` is always available there.

use hwpforge_core::document::Document;
use hwpforge_core::Draft;
use hwpforge_smithy_hwpx::{ExportedDocument, ExportedSection};
use schemars::schema_for;
use serde::{Deserialize, Serialize};

use super::OpsError;

/// Which schema [`schema`] should produce.
///
/// A plain enum, not `#[non_exhaustive]`: a new kind is a new arm the
/// compiler should force every caller in this workspace to handle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SchemaKind {
    /// The Core document tree — what `from_json` accepts.
    #[default]
    Document,
    /// A whole-document export — what `to_json` produces.
    ExportedDocument,
    /// A single-section export — what `export_section` produces and
    /// `patch` consumes.
    ExportedSection,
}

impl SchemaKind {
    /// The wire spelling the CLI and the FFI use (`"exported-section"`, …).
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::ExportedDocument => "exported-document",
            Self::ExportedSection => "exported-section",
        }
    }

    /// Parses a wire spelling, or `None` when no kind has that name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "document" => Some(Self::Document),
            "exported-document" => Some(Self::ExportedDocument),
            "exported-section" => Some(Self::ExportedSection),
            _ => None,
        }
    }
}

/// Options for [`schema`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SchemaOptions {
    /// Which schema to produce.
    pub kind: SchemaKind,
}

impl SchemaOptions {
    /// Selects the schema to produce.
    #[must_use]
    pub fn with_kind(mut self, kind: SchemaKind) -> Self {
        self.kind = kind;
        self
    }

    /// Selects the schema by its wire spelling.
    ///
    /// # Errors
    ///
    /// [`OpsError::InvalidInput`] when no kind has that name. The message
    /// lists the valid spellings, like the CLI's `UNKNOWN_SCHEMA_TYPE` hint.
    pub fn with_kind_name(self, name: &str) -> Result<Self, OpsError> {
        let kind = SchemaKind::parse(name).ok_or_else(|| OpsError::InvalidInput {
            reason: format!(
                "unknown schema type '{name}' — available types: document, \
                 exported-document, exported-section"
            ),
        })?;
        Ok(self.with_kind(kind))
    }
}

/// What [`schema`] returns.
///
/// There is no `Meta` wrapper and no `warnings`: the schema *is* the
/// payload, and the FFI hands the value straight back as the result
/// dictionary.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SchemaOutput {
    /// The JSON Schema document.
    pub schema: serde_json::Value,
}

/// Produces the published JSON Schema for one exchange type.
///
/// # Why the schema is patched
///
/// `to_json` annotates every table cell with its pre-merge grid address
/// (`addr`), and it does so *after* serde has serialised the cell — Core's
/// structs stay free of the projection. The derived schema therefore does
/// not know the property exists, and a consumer validating real `to_json`
/// output against the raw derived schema would reject it. So the same
/// `addr` property the CLI injects is injected here, into the `TableCell`
/// definition of whichever schema was asked for.
///
/// # Errors
///
/// [`OpsError::Json`] if a derived schema cannot be turned into a
/// `serde_json::Value`. `schemars` does not produce such a schema for these
/// types today; the error exists so that a future derive change surfaces
/// instead of panicking.
///
/// # Examples
///
/// ```
/// # #[cfg(all(feature = "ops-hwpx", feature = "schemars"))] {
/// use hwpforge::ops::schema::{schema, SchemaKind, SchemaOptions};
///
/// let out = schema(&SchemaOptions::default().with_kind(SchemaKind::Document))?;
/// assert!(out.schema.get("$defs").is_some());
/// # }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn schema(opts: &SchemaOptions) -> Result<SchemaOutput, OpsError> {
    let mut schema = match opts.kind {
        SchemaKind::Document => serde_json::to_value(schema_for!(Document<Draft>))?,
        SchemaKind::ExportedDocument => serde_json::to_value(schema_for!(ExportedDocument))?,
        SchemaKind::ExportedSection => serde_json::to_value(schema_for!(ExportedSection))?,
    };
    inject_cell_addr(&mut schema);
    Ok(SchemaOutput { schema })
}

/// Documents the `addr` field the export projector adds to table cells.
///
/// A no-op when the schema has no `TableCell` definition, which is what the
/// CLI does too — a schema without table cells needs no annotation.
fn inject_cell_addr(schema: &mut serde_json::Value) {
    let Some(cell) = schema
        .get_mut("$defs")
        .and_then(|defs| defs.get_mut("TableCell"))
        .and_then(|cell| cell.get_mut("properties"))
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    cell.insert(
        "addr".to_string(),
        serde_json::json!({
            "description": "Pre-merge logical grid anchor of this cell (0-based), \
                            added by to-json when the table tiles a well-formed grid. \
                            Optional on import: absent = unchecked, present = must \
                            match the derived grid.",
            "type": "object",
            "properties": {
                "row": { "type": "integer", "format": "uint32", "minimum": 0 },
                "col": { "type": "integer", "format": "uint32", "minimum": 0 }
            },
            "required": ["row", "col"]
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_spellings_round_trip() {
        for kind in
            [SchemaKind::Document, SchemaKind::ExportedDocument, SchemaKind::ExportedSection]
        {
            assert_eq!(SchemaKind::parse(kind.as_str()), Some(kind), "{}", kind.as_str());
        }
    }

    #[test]
    fn serde_uses_the_same_spelling_as_as_str() {
        // The FFI accepts the wire spelling as a string and `SchemaKind`
        // also travels inside serde payloads; the two must not drift.
        let json = serde_json::to_value(SchemaKind::ExportedSection).expect("serialise");
        assert_eq!(json, serde_json::json!("exported-section"));
    }

    #[test]
    fn an_unknown_spelling_is_invalid_input_and_lists_the_valid_ones() {
        let err = SchemaOptions::default().with_kind_name("style").expect_err("must reject");

        assert_eq!(err.code().as_str(), "INVALID_INPUT", "{err}");
        assert!(err.to_string().contains("exported-section"), "{err}");
    }

    #[test]
    fn injection_leaves_a_schema_without_table_cells_alone() {
        let mut value = serde_json::json!({ "$defs": { "Other": { "properties": {} } } });
        let before = value.clone();

        inject_cell_addr(&mut value);

        assert_eq!(value, before, "nothing to annotate, nothing changed");
    }

    #[test]
    fn injection_is_idempotent() {
        let first = schema(&SchemaOptions::default()).expect("schema");
        let mut twice = first.schema.clone();
        inject_cell_addr(&mut twice);

        assert_eq!(twice, first.schema, "re-injecting must not change the schema");
    }
}
