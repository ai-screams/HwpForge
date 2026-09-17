//! Drift audit: no upstream variant that `ops` classifies goes unnamed.
//!
//! # What this catches that the unit tests cannot
//!
//! `src/ops/tests.rs` pins the mapping by constructing one value per variant
//! and asserting its code. That catches a *changed* arm. It cannot catch a
//! *new* upstream variant: every enum `ops` classifies is `#[non_exhaustive]`
//! and none exposes an `ALL`, so the classifiers end in a wildcard and the
//! hand-written case lists simply would not mention the newcomer. A
//! twenty-third `PdfWarning` would be reported as `OTHER`, a twenty-first
//! `PdfError` would lose its `kind`/`location`, and every test would stay
//! green.
//!
//! So this test does not call the mapping at all. It **parses the upstream
//! source** with `syn` and compares the variants it finds against the literal
//! tables below, which mirror `src/ops/mod.rs`. Adding a variant upstream
//! fails here until somebody decides what it maps to.
//!
//! This is the convert-local counterpart of the umbrella's
//! `crates/hwpforge/tests/ops_error_inventory.rs`, deliberately self-contained
//! (no shared support module, no tracked JSON): convert cannot depend on the
//! umbrella, and the five tables here are small enough to read in one screen.
//!
//! # Reading the tables
//!
//! Each row is `(variant, the wire code ops reports)`. `OTHER` is a real
//! answer, not a gap: `ops` deliberately reports the CLI's `OTHER` for warning
//! variants it has no dedicated wording for. Writing those rows out is the
//! point — a new variant cannot hide among them.
//!
//! Parsing is `syn`, never a regex: variant shapes and attributes are read the
//! way the compiler reads them, and **every syntactic variant counts**
//! regardless of `cfg`, so an audit run on one platform cannot miss a variant
//! that exists on another.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hwpforge_convert::ops::PdfCause;
use syn::visit::Visit;
use syn::ItemEnum;

// ── the manifest ────────────────────────────────────────────────

/// One upstream enum the audit covers.
struct Audited {
    /// Path relative to the workspace root.
    file: &'static str,
    /// Enum identifier.
    enum_name: &'static str,
    /// `(variant, wire code)` for every variant that must exist.
    table: &'static [(&'static str, &'static str)],
}

/// Everything `ops` classifies, and what it classifies it as.
///
/// The order is the order `src/ops/mod.rs` handles them in.
const AUDITED: &[Audited] = &[
    Audited {
        // `Hwp5Warning` lives in the decoder, not in `error.rs` beside
        // `Hwp5ErrorCode` — the file each enum is actually declared in is what
        // the audit has to open.
        file: "crates/hwpforge-smithy-hwp5/src/decoder/mod.rs",
        enum_name: "Hwp5Warning",
        table: &[
            ("UnsupportedTag", "UNSUPPORTED_TAG"),
            ("SkippedStream", "SKIPPED_STREAM"),
            ("DroppedControl", "DROPPED_CONTROL"),
            ("ProjectionFallback", "PROJECTION_FALLBACK"),
            ("ParserFallback", "PARSER_FALLBACK"),
            ("LayoutCacheDropped", "LAYOUT_CACHE_DROPPED"),
        ],
    },
    Audited {
        file: "crates/hwpforge-smithy-hwp5/src/error.rs",
        enum_name: "Hwp5ErrorCode",
        table: &[
            ("NotHwp5", "HWP5_DECODE_FAILED"),
            ("Cfb", "HWP5_DECODE_FAILED"),
            ("MissingStream", "HWP5_DECODE_FAILED"),
            ("RecordParse", "HWP5_DECODE_FAILED"),
            ("UnsupportedVersion", "HWP5_DECODE_FAILED"),
            ("PasswordProtected", "HWP5_DECODE_FAILED"),
            ("Encoding", "HWP5_DECODE_FAILED"),
            ("Io", "HWP5_DECODE_FAILED"),
            // The decode stage keeps the per-variant table: a Core error out
            // of the decoder means the file was read and the projection failed.
            ("Core", "HWP5_CONVERT_FAILED"),
            ("Foundation", "HWP5_DECODE_FAILED"),
        ],
    },
    Audited {
        file: "crates/hwpforge-smithy-hwpx/src/encoder/mod.rs",
        enum_name: "EncodeWarning",
        table: &[
            ("LayoutCacheDropped", "LAYOUT_CACHE_DROPPED"),
            // OTHER by design: these three carry no CLI wording, so `ops`
            // keeps their Debug text under the CLI's own `OTHER` spelling
            // rather than inventing a code the frontends do not know.
            ("NoteHeadSkipped", "OTHER"),
            ("TitleMarkSkipped", "OTHER"),
            ("NoteRestartIgnored", "OTHER"),
        ],
    },
    Audited {
        file: "crates/hwpforge-smithy-hwpx/src/decoder/mod.rs",
        enum_name: "DecodeWarning",
        table: &[("UnknownEnumValue", "UNKNOWN_ENUM_VALUE"), ("LayoutCacheDropped", "OTHER")],
    },
    Audited {
        file: "crates/hwpforge-smithy-pdf/src/lib.rs",
        enum_name: "PdfWarning",
        table: &[
            ("ParagraphSkipped", "PARAGRAPH_SKIPPED"),
            ("PageEventLost", "PAGE_EVENT_LOST"),
            ("FontStyleFallback", "FONT_STYLE_FALLBACK"),
            ("ImageDataMissing", "IMAGE_DATA_MISSING"),
            ("UnsupportedImageFormat", "UNSUPPORTED_IMAGE_FORMAT"),
            ("ImageDecodeFailed", "IMAGE_DECODE_FAILED"),
            ("InvalidImageGeometry", "INVALID_IMAGE_GEOMETRY"),
            ("FontAxisFallback", "FONT_AXIS_FALLBACK"),
            ("FontEmbedPreviewPrint", "FONT_EMBED_PREVIEW_PRINT"),
            ("AlignmentApproximated", "ALIGNMENT_APPROXIMATED"),
            ("NonTextRunDropped", "NON_TEXT_RUN_DROPPED"),
            ("AnchorMarkerOnLineBoundary", "ANCHOR_MARKER_ON_LINE_BOUNDARY"),
            ("TablePaginationComputed", "TABLE_PAGINATION_COMPUTED"),
            ("TableDeficitDistributed", "TABLE_DEFICIT_DISTRIBUTED"),
            ("UnsupportedTableStyle", "UNSUPPORTED_TABLE_STYLE"),
            ("BandOverflow", "BAND_OVERFLOW"),
            ("PageStartsOnFallback", "PAGE_STARTS_ON_FALLBACK"),
            ("VertAlignFallback", "VERT_ALIGN_FALLBACK"),
            ("PageNumberSkipped", "PAGE_NUMBER_SKIPPED"),
            ("PageNumberStyleFallback", "PAGE_NUMBER_STYLE_FALLBACK"),
            ("MissingGlyphs", "MISSING_GLYPHS"),
            ("LineOverflow", "LINE_OVERFLOW"),
        ],
    },
    Audited {
        file: "crates/hwpforge-smithy-pdf/src/lib.rs",
        enum_name: "PdfErrorCode",
        table: PDF_ERROR_CAUSE_TABLE,
    },
    Audited {
        // `PdfError` is audited beside its code enum because
        // `ConvertOpsError::cause_info` reads `kind`/`location` off the
        // *variant*, in a match that needs a wildcard for a foreign
        // `#[non_exhaustive]` enum. This row is what makes that wildcard
        // visible: a new variant fails here instead of silently losing its
        // kind and location.
        file: "crates/hwpforge-smithy-pdf/src/lib.rs",
        enum_name: "PdfError",
        table: PDF_ERROR_CAUSE_TABLE,
    },
];

/// The renderer's cause codes. `PdfError` and `PdfErrorCode` are one-to-one,
/// so the two rows share a table.
const PDF_ERROR_CAUSE_TABLE: &[(&str, &str)] = &[
    ("NoRenderableCache", "NO_RENDERABLE_CACHE"),
    ("MissingLayoutCache", "MISSING_LAYOUT_CACHE"),
    ("UnsupportedContent", "UNSUPPORTED_CONTENT"),
    ("InternalInvariant", "INTERNAL_INVARIANT"),
    ("GlyphsUnavailable", "GLYPHS_UNAVAILABLE"),
    ("AmbiguousHeaderFooter", "AMBIGUOUS_HEADER_FOOTER"),
    ("FontUnresolved", "FONT_UNRESOLVED"),
    ("FontStyleUnavailable", "FONT_STYLE_UNAVAILABLE"),
    ("ImageDataMissing", "IMAGE_DATA_MISSING"),
    ("UnsupportedImageFormat", "UNSUPPORTED_IMAGE_FORMAT"),
    ("ImageDecodeFailed", "IMAGE_DECODE_FAILED"),
    ("InvalidImageGeometry", "INVALID_IMAGE_GEOMETRY"),
    ("ImageAssetConflict", "IMAGE_ASSET_CONFLICT"),
    ("FontAxisMismatch", "FONT_AXIS_MISMATCH"),
    ("FontEmbedRestricted", "FONT_EMBED_RESTRICTED"),
    ("FontFaceAmbiguous", "FONT_FACE_AMBIGUOUS"),
    ("InvalidCache", "INVALID_CACHE"),
    ("FontIo", "FONT_IO"),
    ("StyleUnavailable", "STYLE_UNAVAILABLE"),
    ("Backend", "BACKEND"),
];

// ── the parser ──────────────────────────────────────────────────

/// The workspace root, derived from this crate's manifest directory.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("workspace root")
}

/// Collects the variant names of one enum, including inline submodules.
struct EnumFinder {
    wanted: &'static str,
    found: Option<Vec<String>>,
}

impl<'ast> Visit<'ast> for EnumFinder {
    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        if node.ident == self.wanted {
            self.found =
                Some(node.variants.iter().map(|variant| variant.ident.to_string()).collect());
        }
        syn::visit::visit_item_enum(self, node);
    }
}

/// Parses `file` and returns the declared variants of `enum_name`.
///
/// # Panics
///
/// When the file cannot be read or parsed, or does not declare the enum —
/// both mean the manifest above is stale, which is exactly what this test is
/// for.
fn variants_of(file: &str, enum_name: &'static str) -> Vec<String> {
    let path = workspace_root().join(file);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("audited file {}: {err}", path.display()));
    let ast = syn::parse_file(&source)
        .unwrap_or_else(|err| panic!("audited file {file} does not parse: {err}"));

    let mut finder = EnumFinder { wanted: enum_name, found: None };
    for item in &ast.items {
        finder.visit_item(item);
    }
    finder
        .found
        .unwrap_or_else(|| panic!("{file} no longer declares {enum_name} — the manifest is stale"))
}

// ── the audit ───────────────────────────────────────────────────

#[test]
fn every_upstream_variant_is_named_in_the_mapping_tables() {
    for audited in AUDITED {
        let declared: BTreeSet<String> =
            variants_of(audited.file, audited.enum_name).into_iter().collect();
        let mapped: BTreeSet<String> =
            audited.table.iter().map(|(variant, _)| (*variant).to_string()).collect();

        let unmapped: Vec<&String> = declared.difference(&mapped).collect();
        assert!(
            unmapped.is_empty(),
            "{} grew {unmapped:?} — decide what ops reports for it, then add the row \
             (src/ops/mod.rs and the table in this file)",
            audited.enum_name
        );

        let stale: Vec<&String> = mapped.difference(&declared).collect();
        assert!(
            stale.is_empty(),
            "{} no longer declares {stale:?} — remove the row and the arm that handles it",
            audited.enum_name
        );
    }
}

#[test]
fn the_tables_are_well_formed() {
    for audited in AUDITED {
        assert!(!audited.table.is_empty(), "{} has an empty table", audited.enum_name);
        let variants: BTreeSet<&str> = audited.table.iter().map(|(variant, _)| *variant).collect();
        assert_eq!(
            variants.len(),
            audited.table.len(),
            "{} lists a variant twice",
            audited.enum_name
        );
        for (variant, code) in audited.table {
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{}::{variant} maps to {code:?}, which is not a SCREAMING_SNAKE wire code",
                audited.enum_name
            );
        }
    }
}

#[test]
fn every_audited_pdf_cause_code_round_trips_through_the_public_wire_form() {
    // Ties the audit to the running code: `PdfCause` refuses a code its build
    // does not know, so a cause code named here that the crate's own table
    // lacks fails now rather than at a caller.
    for (variant, code) in PDF_ERROR_CAUSE_TABLE {
        let wire = serde_json::json!({ "stage": "render", "code": code });
        let parsed: PdfCause = serde_json::from_value(wire)
            .unwrap_or_else(|err| panic!("PdfError::{variant} → {code}: {err}"));
        assert_eq!(parsed.code.as_str(), *code);
        assert_eq!(parsed.stage, "render");
    }
}
