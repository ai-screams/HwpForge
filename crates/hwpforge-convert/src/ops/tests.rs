//! Unit tests for the classification tables.
//!
//! The pipeline itself is covered end-to-end by `tests/ops_convert_hwp5.rs`
//! and `tests/ops_to_pdf.rs`. What lives here is the part an end-to-end test
//! cannot reach: most [`PdfError`] and [`PdfWarning`] variants need a
//! document crafted to provoke them, so the tables are pinned by
//! constructing each variant directly and asserting the code it maps to.
//!
//! # What these tests do and do not catch
//!
//! They catch a **changed** mapping: edit an arm and a case fails. They do
//! **not** catch a **new** upstream variant. Every enum classified here is
//! `#[non_exhaustive]` and none exposes an `ALL` const, so this crate cannot
//! enumerate the variants that exist — the case lists are hand-written, and a
//! twenty-third [`PdfWarning`] would be absorbed by the `_` arm as `OTHER`
//! with every test still green.
//!
//! That half is `tests/ops_inventory.rs`, which parses the upstream enums
//! with `syn` and fails when one grows a variant the tables here do not name.
//! The two are complements: the inventory proves the tables are *complete*,
//! these tests prove they are *right*.

use std::path::PathBuf;

use hwpforge_core::CoreError;
use hwpforge_smithy_hwpx::{ParagraphPath, PathSeg};
use hwpforge_smithy_pdf::font::FaceStyle;

use super::*;
use crate::LayoutPatchError;

fn path() -> ParagraphPath {
    ParagraphPath(vec![PathSeg::Section(0)])
}

// ── errors ──────────────────────────────────────────────────────

#[test]
fn every_hwp5_error_code_is_classified() {
    // One error per `Hwp5ErrorCode`, so the table has no unvisited arm.
    let cases: Vec<(Hwp5Error, OpsCode)> = vec![
        (Hwp5Error::NotHwp5 { detail: "no magic".into() }, OpsCode::Hwp5DecodeFailed),
        (Hwp5Error::Cfb { detail: "bad OLE header".into() }, OpsCode::Hwp5DecodeFailed),
        (Hwp5Error::MissingStream { name: "BodyText/Section0".into() }, OpsCode::Hwp5DecodeFailed),
        (
            Hwp5Error::RecordParse { detail: "truncated TLV".into(), offset: 12 },
            OpsCode::Hwp5DecodeFailed,
        ),
        (
            Hwp5Error::UnsupportedVersion { major: 3, minor: 0, micro: 0, build: 0 },
            OpsCode::Hwp5DecodeFailed,
        ),
        (Hwp5Error::PasswordProtected, OpsCode::Hwp5DecodeFailed),
        (Hwp5Error::Encoding { detail: "bad UTF-16LE".into() }, OpsCode::Hwp5DecodeFailed),
        (Hwp5Error::Io(std::io::Error::other("cursor")), OpsCode::Hwp5DecodeFailed),
        (
            Hwp5Error::Foundation(hwpforge_foundation::FoundationError::InvalidHwpUnit {
                value: -1,
                min: 0,
                max: i32::MAX,
            }),
            OpsCode::Hwp5DecodeFailed,
        ),
        // Produced at exactly one place: convert's own `validate()`.
        (
            Hwp5Error::Core(CoreError::InvalidStructure {
                context: "document".into(),
                reason: "no sections".into(),
            }),
            OpsCode::Hwp5ConvertFailed,
        ),
    ];

    let mut seen: Vec<Hwp5ErrorCode> = Vec::new();
    for (error, expected) in cases {
        let wrapped = ConvertOpsError::Convert(ConvertError::Decode(error));
        assert_eq!(wrapped.code(), expected, "{wrapped}");
        assert!(!wrapped.to_string().is_empty());
        assert!(wrapped.cause().is_none(), "only a render failure has a cause");
        assert!(wrapped.cause_info().is_none(), "…and that includes the detailed cause");
        let ConvertOpsError::Convert(ConvertError::Decode(inner)) = &wrapped else {
            unreachable!()
        };
        seen.push(inner.code());
    }
    seen.sort_by_key(|code| *code as u16);
    seen.dedup();
    assert_eq!(seen.len(), 10, "one case per known Hwp5ErrorCode");
}

#[test]
fn every_post_decode_stage_reports_convert_failed() {
    // The finding this fixes: an HWPX encode failure and a layout-patch
    // failure used to arrive as `Hwp5Error::Cfb`/`MissingStream` and so
    // claimed the *source file* could not be read.
    //
    // Why by construction and not from bytes: none of the three post-decode
    // stages is reachable from an input this suite can supply. `Validate`
    // needs an HWP5 file that decodes cleanly and then fails Core validation;
    // `Encode` needs the HWPX encoder to refuse a document the decoder just
    // produced; and `LayoutPatch` only ever inspects a package *this crate
    // generated moments earlier*, so reaching it means the encoder and the
    // patcher disagree. The decode stage is the one with a byte-level test —
    // `tests/ops_convert_hwp5.rs` drives it with garbage, a ZIP and a
    // truncated container.
    let cases: Vec<(ConvertError, OpsCode, &str)> = vec![
        (
            ConvertError::Validate(CoreError::InvalidStructure {
                context: "document".into(),
                reason: "no sections".into(),
            }),
            OpsCode::Hwp5ConvertFailed,
            "validate",
        ),
        (
            ConvertError::Encode(HwpxError::XmlSerialize { detail: "section0.xml".into() }),
            OpsCode::Hwp5ConvertFailed,
            "encode",
        ),
        (
            ConvertError::LayoutPatch(LayoutPatchError::Package {
                detail: "finish hwpx patch package: io".into(),
            }),
            OpsCode::Hwp5ConvertFailed,
            "layout-patch",
        ),
        (
            ConvertError::LayoutPatch(LayoutPatchError::MissingEntry {
                name: "Contents/section3.xml".into(),
            }),
            OpsCode::Hwp5ConvertFailed,
            "layout-patch",
        ),
    ];

    for (error, expected, stage) in cases {
        assert_eq!(error.stage(), stage);
        let wrapped = ConvertOpsError::Convert(error);
        assert_eq!(wrapped.code(), expected, "{wrapped}");
        assert_eq!(wrapped.code().as_str(), "HWP5_CONVERT_FAILED");
        assert!(wrapped.hint().is_some(), "the caller is told what to check");
    }
}

#[test]
fn a_decode_stage_core_error_still_reports_convert_failed() {
    // The decode stage keeps the per-variant table rather than flattening to
    // one code: a Core error that propagates out of the decoder means the
    // container was read and the projection is what failed.
    let error = ConvertOpsError::Convert(ConvertError::Decode(Hwp5Error::Core(
        CoreError::InvalidStructure { context: "document".into(), reason: "no sections".into() },
    )));
    assert_eq!(error.code(), OpsCode::Hwp5ConvertFailed);
}

#[test]
fn crate_owned_variants_carry_their_own_codes() {
    let cases: Vec<(ConvertOpsError, OpsCode)> = vec![
        (
            ConvertOpsError::Decode(HwpxError::InvalidMimetype { actual: "text/plain".into() }),
            OpsCode::DecodeFailed,
        ),
        (
            ConvertOpsError::Core(CoreError::InvalidStructure {
                context: "document".into(),
                reason: "empty".into(),
            }),
            OpsCode::ValidationFailed,
        ),
        (
            ConvertOpsError::Pdf(PdfError::NoRenderableCache { section: 0 }),
            OpsCode::PdfRenderFailed,
        ),
        (ConvertOpsError::UnrecognizedFormat, OpsCode::UnrecognizedFormat),
        // Nothing in this crate builds `Rejected`; a frontend does, to report
        // its own pre-library rejection through this error type.
        (
            ConvertOpsError::Rejected {
                code: OpsCode::InvalidDiscovery,
                reason: "unknown discovery mode 'sometimes'".into(),
            },
            OpsCode::InvalidDiscovery,
        ),
    ];

    for (error, expected) in cases {
        assert_eq!(error.code(), expected, "{error}");
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn only_a_render_failure_exposes_the_renderer_code() {
    let render = ConvertOpsError::Pdf(PdfError::NoRenderableCache { section: 3 });
    assert_eq!(render.cause(), Some(PdfErrorCode::NoRenderableCache));
    assert_eq!(render.cause().map(PdfErrorCode::as_str), Some("NO_RENDERABLE_CACHE"));
    // The coarse accessor is the detailed one's `code` — one source of truth.
    assert_eq!(render.cause(), render.cause_info().map(|cause| cause.code));

    for error in [
        ConvertOpsError::UnrecognizedFormat,
        ConvertOpsError::Decode(HwpxError::InvalidMimetype { actual: "x".into() }),
        ConvertOpsError::Rejected {
            code: OpsCode::InvalidDiscovery,
            reason: "unknown discovery mode".into(),
        },
    ] {
        assert!(error.cause().is_none(), "{error}");
        assert!(error.cause_info().is_none(), "{error}");
    }
}

/// The CLI's `pdf_error_cause` expectations, variant for variant.
///
/// Copied from `hwpforge-bindings-cli`'s `pdf_error_cause_maps_every_variant`
/// so the two mappers cannot drift: `cause_info()` exists precisely so W3 can
/// delete the CLI's own 20-arm mapper and print this instead.
fn cli_cause_cases() -> Vec<(PdfError, &'static str, Option<&'static str>, Option<&'static str>)> {
    let loc = || "s0/p1/t0r0c0".to_string();
    vec![
        (PdfError::NoRenderableCache { section: 2 }, "NO_RENDERABLE_CACHE", None, Some("s2")),
        (
            PdfError::MissingLayoutCache { count: 3, first: "s0/p7".into() },
            "MISSING_LAYOUT_CACHE",
            None,
            Some("s0/p7"),
        ),
        (
            PdfError::UnsupportedContent { kind: "table mixed with inline image", location: loc() },
            "UNSUPPORTED_CONTENT",
            Some("table mixed with inline image"),
            Some("s0/p1/t0r0c0"),
        ),
        (PdfError::InternalInvariant { detail: "d".into() }, "INTERNAL_INVARIANT", None, None),
        (
            PdfError::GlyphsUnavailable { face: "f".into(), count: 1, location: loc() },
            "GLYPHS_UNAVAILABLE",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::AmbiguousHeaderFooter { kind: "header", detail: "d".into() },
            "AMBIGUOUS_HEADER_FOOTER",
            Some("header"),
            None,
        ),
        (PdfError::FontUnresolved { face: "f".into() }, "FONT_UNRESOLVED", None, None),
        (
            PdfError::FontStyleUnavailable {
                face: "f".into(),
                style: FaceStyle::Bold,
                location: loc(),
            },
            "FONT_STYLE_UNAVAILABLE",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::ImageDataMissing { key: "k".into(), location: loc() },
            "IMAGE_DATA_MISSING",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::UnsupportedImageFormat { key: "k".into(), format: "Bmp", location: loc() },
            "UNSUPPORTED_IMAGE_FORMAT",
            Some("Bmp"),
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::ImageDecodeFailed { key: "k".into(), detail: "d".into(), location: loc() },
            "IMAGE_DECODE_FAILED",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::InvalidImageGeometry { key: "k".into(), detail: "d".into(), location: loc() },
            "INVALID_IMAGE_GEOMETRY",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::ImageAssetConflict { key: "k".into(), location: loc() },
            "IMAGE_ASSET_CONFLICT",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::FontAxisMismatch { location: loc(), fonts: vec!["a".into()] },
            "FONT_AXIS_MISMATCH",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (
            PdfError::FontEmbedRestricted {
                face: "f".into(),
                path: "/x".into(),
                reason: "r".into(),
            },
            "FONT_EMBED_RESTRICTED",
            None,
            None,
        ),
        (
            PdfError::FontFaceAmbiguous {
                face: "f".into(),
                style: FaceStyle::Bold,
                detail: "d".into(),
            },
            "FONT_FACE_AMBIGUOUS",
            None,
            None,
        ),
        (PdfError::InvalidCache { detail: "d".into() }, "INVALID_CACHE", None, None),
        (
            PdfError::FontIo(std::io::Error::new(std::io::ErrorKind::NotFound, "x")),
            "FONT_IO",
            None,
            None,
        ),
        (
            PdfError::StyleUnavailable { what: "font name", location: loc() },
            "STYLE_UNAVAILABLE",
            None,
            Some("s0/p1/t0r0c0"),
        ),
        (PdfError::Backend("b".into()), "BACKEND", None, None),
    ]
}

#[test]
fn cause_info_reproduces_the_cli_cause_for_every_variant() {
    let cases = cli_cause_cases();
    assert_eq!(cases.len(), 20, "one case per known PdfError variant");

    for (error, code, kind, location) in cases {
        let cause = ConvertOpsError::Pdf(error).cause_info().expect("a render failure has a cause");
        assert_eq!(cause.stage, "render");
        assert_eq!(cause.code.as_str(), code);
        assert_eq!(cause.kind.as_deref(), kind, "{code} kind");
        assert_eq!(cause.location.as_deref(), location, "{code} location");
    }
}

#[test]
fn a_cause_round_trips_through_its_wire_form() {
    // The wire shape is the CLI's `cause` block: an absent kind or location is
    // omitted, not serialised as null.
    let cause =
        ConvertOpsError::Pdf(PdfError::MissingLayoutCache { count: 3, first: "s0/p7".into() })
            .cause_info()
            .expect("a cause");

    let value = serde_json::to_value(&cause).expect("serialise");
    let mut keys: Vec<&String> = value.as_object().expect("an object").keys().collect();
    keys.sort();
    assert_eq!(keys, vec!["code", "location", "stage"], "an absent kind is omitted");
    assert_eq!(value["code"], "MISSING_LAYOUT_CACHE", "the stable string, not a Rust name");

    let parsed: PdfCause = serde_json::from_value(value).expect("deserialise");
    assert_eq!(parsed, cause);

    // The other side of `skip_serializing_if`: a cause that has a kind keeps
    // it, because `kind` is what separates two refusals sharing one code.
    let with_kind = ConvertOpsError::Pdf(PdfError::UnsupportedContent {
        kind: "table mixed with inline image",
        location: "s0/p1/t0r0c0".into(),
    })
    .cause_info()
    .expect("a cause");
    let value = serde_json::to_value(&with_kind).expect("serialise");
    assert_eq!(value["kind"], "table mixed with inline image");
    assert_eq!(serde_json::from_value::<PdfCause>(value).expect("deserialise"), with_kind);

    // A code this build does not know is refused rather than defaulted: a
    // cause that guesses is worse than no cause.
    let unknown = serde_json::json!({ "stage": "render", "code": "FROM_THE_FUTURE" });
    assert!(serde_json::from_value::<PdfCause>(unknown).is_err());
    let unknown_stage = serde_json::json!({ "stage": "shape", "code": "BACKEND" });
    assert!(serde_json::from_value::<PdfCause>(unknown_stage).is_err());
}

#[test]
fn every_known_pdf_error_code_is_named_in_the_wire_table() {
    // `KNOWN_PDF_ERROR_CODES` is what makes the wire form round-trip; the
    // samples above are the proof that it covers what `code()` can return.
    for (error, code, _, _) in cli_cause_cases() {
        assert!(
            KNOWN_PDF_ERROR_CODES.contains(&error.code()),
            "{code} is missing from KNOWN_PDF_ERROR_CODES"
        );
    }
    assert_eq!(KNOWN_PDF_ERROR_CODES.len(), 20);
}

#[test]
fn hints_are_present_where_the_cli_prints_one_and_never_blank() {
    for code in OpsCode::ALL {
        if let Some(hint) = hint_for(*code) {
            assert!(!hint.trim().is_empty(), "{code} has a blank hint");
        }
    }
    // The classes this crate can actually return all carry a recovery hint —
    // a frontend has something to print for every failure it sees.
    for code in [
        OpsCode::Hwp5DecodeFailed,
        OpsCode::Hwp5ConvertFailed,
        OpsCode::DecodeFailed,
        OpsCode::UnrecognizedFormat,
        OpsCode::PdfRenderFailed,
        OpsCode::UpstreamUnmapped,
    ] {
        assert!(hint_for(code).is_some(), "{code} needs a hint");
    }
    // Validation failures quote the offending structure in their message, so
    // there is no useful static hint to add.
    assert!(hint_for(OpsCode::ValidationFailed).is_none());
}

// ── warnings ────────────────────────────────────────────────────

#[test]
fn convert_warnings_keep_the_cli_codes_and_the_convert_stage() {
    let cases: Vec<(ConvertWarning, &str, &str)> = vec![
        (
            ConvertWarning::Hwp5(Hwp5Warning::UnsupportedTag { tag_id: 0x5B, offset: 12 }),
            "UNSUPPORTED_TAG",
            "offset 12: unsupported record tag 0x5B",
        ),
        (
            ConvertWarning::Hwp5(Hwp5Warning::SkippedStream { name: "Scripts".into() }),
            "SKIPPED_STREAM",
            "stream skipped: Scripts",
        ),
        (
            ConvertWarning::Hwp5(Hwp5Warning::DroppedControl {
                control: "ole_object",
                reason: "no fixture".into(),
            }),
            "DROPPED_CONTROL",
            "ole_object: no fixture",
        ),
        (
            ConvertWarning::Hwp5(Hwp5Warning::ProjectionFallback {
                subject: "tab",
                reason: "slot 3".into(),
            }),
            "PROJECTION_FALLBACK",
            "tab: slot 3",
        ),
        (
            ConvertWarning::Hwp5(Hwp5Warning::ParserFallback {
                subject: "numbering.slot",
                reason: "slot 1".into(),
            }),
            "PARSER_FALLBACK",
            "numbering.slot: slot 1",
        ),
        (
            ConvertWarning::Hwp5(Hwp5Warning::LayoutCacheDropped { reason: "ledger".into() }),
            "LAYOUT_CACHE_DROPPED",
            "ledger",
        ),
        (
            ConvertWarning::HwpxEncode(EncodeWarning::LayoutCacheDropped {
                path: path(),
                reason: "ledger".into(),
            }),
            "LAYOUT_CACHE_DROPPED",
            "section[0]: ledger",
        ),
    ];

    for (warning, code, message) in cases {
        let wrapped = ConvertOpsWarning::Convert(warning);
        assert_eq!(wrapped.stage(), "convert");
        let info = wrapped.info();
        assert_eq!(info.code, code);
        assert_eq!(info.message, message);
    }
}

#[test]
fn an_unclassified_encode_warning_reports_other_without_losing_its_text() {
    let warning =
        ConvertOpsWarning::Convert(ConvertWarning::HwpxEncode(EncodeWarning::NoteHeadSkipped {
            path: path(),
            reason: "titleMark".into(),
        }));
    let info = warning.info();
    assert_eq!(info.code, "OTHER", "the CLI's spelling for an unknown variant");
    assert!(info.message.contains("NoteHeadSkipped"), "{}", info.message);
    assert!(info.message.contains("titleMark"), "{}", info.message);
}

#[test]
fn decode_warnings_keep_the_attribute_and_the_decode_stage() {
    let warning = ConvertOpsWarning::Decode(DecodeWarning::UnknownEnumValue {
        attribute: "hp:header@applyPageType",
        raw: "WEIRD".into(),
        fallback: "BOTH",
    });
    assert_eq!(warning.stage(), "decode");
    let info = warning.info();
    assert_eq!(info.code, "UNKNOWN_ENUM_VALUE");
    assert_eq!(info.message, "hp:header@applyPageType: \"WEIRD\" unknown — fell back to BOTH");
}

#[test]
fn every_known_render_warning_has_a_code_and_the_render_stage() {
    let location = || "s0/p1".to_string();
    let cases: Vec<(PdfWarning, &str)> = vec![
        (PdfWarning::ParagraphSkipped { location: location() }, "PARAGRAPH_SKIPPED"),
        (PdfWarning::PageEventLost { location: location() }, "PAGE_EVENT_LOST"),
        (
            PdfWarning::FontStyleFallback {
                face: "함초롬바탕".into(),
                requested: FaceStyle::Bold,
                location: location(),
            },
            "FONT_STYLE_FALLBACK",
        ),
        (
            PdfWarning::FontAxisFallback { fonts: vec!["A".into()], location: location() },
            "FONT_AXIS_FALLBACK",
        ),
        (
            PdfWarning::FontEmbedPreviewPrint {
                face: "A".into(),
                path: PathBuf::from("/fonts/a.ttf"),
                fingerprint: "deadbeef".into(),
            },
            "FONT_EMBED_PREVIEW_PRINT",
        ),
        (PdfWarning::AlignmentApproximated { location: location() }, "ALIGNMENT_APPROXIMATED"),
        (PdfWarning::NonTextRunDropped { location: location() }, "NON_TEXT_RUN_DROPPED"),
        (
            PdfWarning::AnchorMarkerOnLineBoundary { location: location() },
            "ANCHOR_MARKER_ON_LINE_BOUNDARY",
        ),
        (
            PdfWarning::ImageDataMissing { key: "image1.png".into(), location: location() },
            "IMAGE_DATA_MISSING",
        ),
        (
            PdfWarning::UnsupportedImageFormat {
                key: "image1.wmf".into(),
                format: "WMF",
                location: location(),
            },
            "UNSUPPORTED_IMAGE_FORMAT",
        ),
        (
            PdfWarning::ImageDecodeFailed {
                key: "image1.png".into(),
                detail: "truncated".into(),
                location: location(),
            },
            "IMAGE_DECODE_FAILED",
        ),
        (
            PdfWarning::InvalidImageGeometry {
                key: "image1.png".into(),
                detail: "zero width".into(),
                location: location(),
            },
            "INVALID_IMAGE_GEOMETRY",
        ),
        (PdfWarning::TablePaginationComputed { location: location() }, "TABLE_PAGINATION_COMPUTED"),
        (PdfWarning::TableDeficitDistributed { location: location() }, "TABLE_DEFICIT_DISTRIBUTED"),
        (
            PdfWarning::UnsupportedTableStyle { location: location(), what: "diagonal" },
            "UNSUPPORTED_TABLE_STYLE",
        ),
        (PdfWarning::BandOverflow { kind: "header", location: location() }, "BAND_OVERFLOW"),
        (PdfWarning::PageStartsOnFallback { section: 0 }, "PAGE_STARTS_ON_FALLBACK"),
        (PdfWarning::VertAlignFallback { location: location() }, "VERT_ALIGN_FALLBACK"),
        (
            PdfWarning::PageNumberSkipped { section: 0, what: "side position" },
            "PAGE_NUMBER_SKIPPED",
        ),
        (PdfWarning::PageNumberStyleFallback { section: 0 }, "PAGE_NUMBER_STYLE_FALLBACK"),
        (
            PdfWarning::MissingGlyphs { face: "A".into(), count: 3, location: location() },
            "MISSING_GLYPHS",
        ),
        (PdfWarning::LineOverflow { location: location(), excess: 120 }, "LINE_OVERFLOW"),
    ];

    assert_eq!(cases.len(), 22, "one case per known PdfWarning variant");
    for (warning, code) in cases {
        let wrapped = ConvertOpsWarning::Render(warning);
        assert_eq!(wrapped.stage(), "render");
        let info = wrapped.info();
        assert_eq!(info.code, code, "{:?}", info.message);
        assert!(!info.message.trim().is_empty(), "{code} has a blank message");
        assert!(info.hint.is_none(), "warnings carry no static hint today");
    }
}

#[test]
fn a_location_is_prefixed_into_the_message_not_dropped() {
    // `WarningInfo` has no location field, so the CLI's separate `location`
    // becomes a prefix. Losing it would make corpus triage guess at paths.
    let info =
        ConvertOpsWarning::Render(PdfWarning::ParagraphSkipped { location: "s2/p17".into() })
            .info();
    assert_eq!(info.message, "s2/p17: paragraph without layout cache skipped");

    // A warning with no location keeps its message verbatim.
    let info = ConvertOpsWarning::Render(PdfWarning::FontEmbedPreviewPrint {
        face: "A".into(),
        path: PathBuf::from("/fonts/a.ttf"),
        fingerprint: "deadbeef".into(),
    })
    .info();
    assert!(!info.message.starts_with(": "), "{}", info.message);
}
