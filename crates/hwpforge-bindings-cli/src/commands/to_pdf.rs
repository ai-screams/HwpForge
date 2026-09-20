//! Render HWPX/HWP5 to PDF (layout-cache replay — W6a).
//!
//! Format dispatch is **content sniffing**, not extension: this repo's own
//! corpus study found 79 HWP5 binaries shipped with `.hwpx` extensions.
//! The extension is only a hint — a mismatch is surfaced as a warning.
//!
//! Warnings keep their provenance across the three pipeline stages
//! (`convert` → `decode` → `render`) as structured DTOs — flattening them
//! would hide HWP5 conversion loss behind a green render.

use std::path::{Path, PathBuf};

use serde::Serialize;

use hwpforge_convert::ops::{to_pdf, ConvertOpsWarning, ToPdfOptions};
use hwpforge_convert::ConvertWarning;
use hwpforge_smithy_hwp5::Hwp5Warning;
use hwpforge_smithy_hwpx::DecodeWarning;
use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::PdfWarning;

use crate::compat::{self, Command};
use crate::error::{check_file_size, CliError};

/// OLE2/CFB magic — HWP5 컨테이너.
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[derive(Serialize)]
struct WarningDto {
    /// 발생 단계: `input`(디스패치) | `convert`(HWP5→HWPX) | `decode`(HWPX 해석)
    /// | `render`(PDF).
    stage: &'static str,
    /// 안정 코드 (variant 유래 — 스크립트 필터링용). `render_warning_dto`'s
    /// 코드는 `ConvertOpsWarning::info()` 에서 오므로 `String` — 나머지
    /// 생성자는 정적 리터럴을 그대로 담는다.
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    location: Option<String>,
}

#[derive(Serialize)]
struct WarningCounts {
    input: usize,
    convert: usize,
    decode: usize,
    render: usize,
}

#[derive(Serialize)]
struct ToPdfResult {
    status: &'static str,
    input: String,
    output: String,
    detected_format: &'static str,
    size_bytes: u64,
    warnings: Vec<WarningDto>,
    warning_counts: WarningCounts,
}

/// 콘텐츠 스니핑 — CFB=HWP5, ZIP=HWPX 후보 (mimetype 은 디코더가 검증).
fn detect_format(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&CFB_MAGIC) {
        return Some("hwp5");
    }
    if bytes.starts_with(b"PK") {
        return Some("hwpx");
    }
    None
}

fn convert_warning_dto(w: &ConvertWarning) -> WarningDto {
    let w = match w {
        ConvertWarning::Hwp5(w) => w,
        // W1b: 인코드 캐시 드롭 — 문단 경로+사유를 그대로 표면화.
        ConvertWarning::HwpxEncode(hwpforge_smithy_hwpx::EncodeWarning::LayoutCacheDropped {
            path,
            reason,
        }) => {
            return WarningDto {
                stage: "convert",
                code: "LAYOUT_CACHE_DROPPED".to_string(),
                message: reason.clone(),
                location: Some(path.to_string()),
            };
        }
        other => {
            return WarningDto {
                stage: "convert",
                code: "OTHER".to_string(),
                message: format!("{other:?}"),
                location: None,
            };
        }
    };
    let (code, message, location) = match w {
        Hwp5Warning::UnsupportedTag { tag_id, offset } => (
            "UNSUPPORTED_TAG",
            format!("unsupported record tag 0x{tag_id:02X}"),
            Some(format!("offset {offset}")),
        ),
        Hwp5Warning::SkippedStream { name } => {
            ("SKIPPED_STREAM", format!("stream skipped: {name}"), None)
        }
        Hwp5Warning::DroppedControl { control, reason } => {
            ("DROPPED_CONTROL", format!("{control}: {reason}"), None)
        }
        Hwp5Warning::ProjectionFallback { subject, reason } => {
            ("PROJECTION_FALLBACK", format!("{subject}: {reason}"), None)
        }
        Hwp5Warning::ParserFallback { subject, reason } => {
            ("PARSER_FALLBACK", format!("{subject}: {reason}"), None)
        }
        Hwp5Warning::LayoutCacheDropped { reason } => {
            ("LAYOUT_CACHE_DROPPED", reason.clone(), None)
        }
        other => ("OTHER", format!("{other:?}"), None),
    };
    WarningDto { stage: "convert", code: code.to_string(), message, location }
}

fn decode_warning_dto(w: &DecodeWarning) -> WarningDto {
    let (code, message, location) = match w {
        DecodeWarning::UnknownEnumValue { attribute, raw, fallback } => (
            "UNKNOWN_ENUM_VALUE",
            format!("\"{raw}\" unknown — fell back to {fallback}"),
            Some((*attribute).to_string()),
        ),
        other => ("OTHER", format!("{other:?}"), None),
    };
    WarningDto { stage: "decode", code: code.to_string(), message, location }
}

/// `code`/`message` come from [`ConvertOpsWarning::info`] — the ops crate's
/// own single source of truth for a `PdfWarning`'s wire shape, which the CLI
/// used to re-derive by hand in a ~118-line match kept in lockstep with it
/// (audit finding). `location` still needs its own lookup below:
/// [`hwpforge_foundation::diagnostics::WarningInfo`] has no `location`
/// field — `info()` folds it into the message text instead — but the CLI
/// DTO keeps `location` as its own column, unchanged from before.
fn render_warning_dto(w: &PdfWarning) -> WarningDto {
    let info = ConvertOpsWarning::Render(w.clone()).info();
    WarningDto {
        stage: "render",
        code: info.code,
        message: info.message,
        location: render_warning_location(w),
    }
}

/// The `location` half of [`render_warning_dto`] — every variant that
/// carries one, grouped by shape (`location: String` vs. `section: usize`
/// formatted as `"s{n}"`); `FontEmbedPreviewPrint` and any future variant
/// have none.
fn render_warning_location(w: &PdfWarning) -> Option<String> {
    match w {
        PdfWarning::ParagraphSkipped { location }
        | PdfWarning::PageEventLost { location }
        | PdfWarning::FontStyleFallback { location, .. }
        | PdfWarning::FontAxisFallback { location, .. }
        | PdfWarning::AlignmentApproximated { location }
        | PdfWarning::NonTextRunDropped { location }
        | PdfWarning::AnchorMarkerOnLineBoundary { location }
        | PdfWarning::ImageDataMissing { location, .. }
        | PdfWarning::UnsupportedImageFormat { location, .. }
        | PdfWarning::ImageDecodeFailed { location, .. }
        | PdfWarning::InvalidImageGeometry { location, .. }
        | PdfWarning::TablePaginationComputed { location }
        | PdfWarning::TableDeficitDistributed { location }
        | PdfWarning::UnsupportedTableStyle { location, .. }
        | PdfWarning::BandOverflow { location, .. }
        | PdfWarning::VertAlignFallback { location }
        | PdfWarning::MissingGlyphs { location, .. }
        | PdfWarning::LineOverflow { location, .. } => Some(location.clone()),
        PdfWarning::PageStartsOnFallback { section }
        | PdfWarning::PageNumberSkipped { section, .. }
        | PdfWarning::PageNumberStyleFallback { section } => Some(format!("s{section}")),
        _ => None,
    }
}

fn parse_discovery(s: &str, json_mode: bool) -> FontDiscovery {
    match s {
        "explicit" => FontDiscovery::ExplicitOnly,
        "hancom" => FontDiscovery::HancomBundle,
        "platform" => FontDiscovery::Platform,
        other => CliError::new(
            "INVALID_DISCOVERY",
            format!("unknown discovery mode '{other}' (expected explicit|hancom|platform)"),
        )
        .exit(json_mode, 2),
    }
}

/// Run the to-pdf command.
#[allow(clippy::too_many_arguments)]
pub fn run(
    input: &Path,
    output: Option<&Path>,
    font_dirs: &[PathBuf],
    discovery: &str,
    degraded: bool,
    partial_cache_reject: bool,
    json_mode: bool,
) {
    // 플래그 오류는 파이프라인 진입 전에 (독립 리뷰 L1 — .hwp 변환 후 exit 방지).
    let discovery = parse_discovery(discovery, json_mode);
    check_file_size(input, json_mode);
    let bytes = crate::error::read_input(input, json_mode);

    let Some(detected) = detect_format(&bytes) else {
        CliError::new(
            "UNRECOGNIZED_FORMAT",
            format!("'{}' is neither an OLE2 (HWP5) nor a ZIP (HWPX) container", input.display()),
        )
        .with_hint("to-pdf detects the format by content — the extension is only a hint")
        .exit(json_mode, 2)
    };

    let mut warnings: Vec<WarningDto> = Vec::new();
    // 확장자는 힌트 — 실물과 다르면 경고 (corpus 실측: .hwpx 탈 HWP5 79건).
    let ext = input.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    let ext_implies = match ext.as_deref() {
        Some("hwp") => Some("hwp5"),
        Some("hwpx") => Some("hwpx"),
        _ => None,
    };
    if let Some(implied) = ext_implies {
        if implied != detected {
            warnings.push(WarningDto {
                stage: "input",
                code: "EXTENSION_MISMATCH".to_string(),
                message: format!("extension implies {implied} but content is {detected}"),
                location: None,
            });
        }
    }

    // HWP5 변환(캐시 carry 고정) → HWPX 디코드 → 검증 → 렌더 — 한 번의 ops
    // 호출로 (`hwpforge_convert::ops::to_pdf`). `detect_format` 은 이미
    // 로컬에서 통과했으므로 ops 내부의 같은 콘텐츠-스니핑은 여기서 절대
    // `UnrecognizedFormat` 을 내지 않는다 — 그 오류의 정확한 레거시
    // 메시지(파일명 포함)는 위 로컬 체크가 이미 낸다.
    let pdf_options = ToPdfOptions::default()
        .with_font_dirs(font_dirs.to_vec())
        .with_discovery(discovery)
        .with_degraded(degraded)
        .with_partial_cache_reject(partial_cache_reject);
    let rendered = to_pdf(&bytes, &pdf_options).unwrap_or_else(|err| {
        let cli_err = compat::convert_error(Command::ToPdf, err);
        let exit = compat::exit_code(Command::ToPdf, &cli_err);
        cli_err.exit(json_mode, exit)
    });
    for w in &rendered.warnings {
        warnings.push(match w {
            ConvertOpsWarning::Convert(inner) => convert_warning_dto(inner),
            ConvertOpsWarning::Decode(inner) => decode_warning_dto(inner),
            ConvertOpsWarning::Render(inner) => render_warning_dto(inner),
            // `ConvertOpsWarning` is `#[non_exhaustive]` — `to_pdf` only ever
            // emits the three staged variants above today.
            _ => WarningDto {
                stage: "render",
                code: "OTHER".to_string(),
                message: format!("{w:?}"),
                location: None,
            },
        });
    }

    // 산출 경로: 미지정 = 입력의 .pdf 교체. 쓰기는 원자적 (tmp → rename) —
    // tmp 이름에 pid 를 넣어 동시 실행 충돌을 피하고, 실패 시 잔여물을 정리한다.
    let out_path = output.map_or_else(|| input.with_extension("pdf"), Path::to_path_buf);
    let tmp_path = out_path.with_extension(format!("pdf.tmp.{}", std::process::id()));
    std::fs::write(&tmp_path, &rendered.bytes)
        .and_then(|()| std::fs::rename(&tmp_path, &out_path))
        .unwrap_or_else(|err| {
            let _ = std::fs::remove_file(&tmp_path);
            CliError::new(
                "FILE_WRITE_FAILED",
                format!("Cannot write '{}': {err}", out_path.display()),
            )
            .exit(json_mode, 1)
        });

    let counts = WarningCounts {
        input: warnings.iter().filter(|w| w.stage == "input").count(),
        convert: warnings.iter().filter(|w| w.stage == "convert").count(),
        decode: warnings.iter().filter(|w| w.stage == "decode").count(),
        render: warnings.iter().filter(|w| w.stage == "render").count(),
    };
    let result = ToPdfResult {
        status: "ok",
        input: input.display().to_string(),
        output: out_path.display().to_string(),
        detected_format: detected,
        size_bytes: rendered.bytes.len() as u64,
        warnings,
        warning_counts: counts,
    };
    if json_mode {
        println!("{}", serde_json::to_string(&result).unwrap());
    } else {
        println!(
            "PDF written: {} ({} bytes, {} 경고 — convert {} · decode {} · render {})",
            result.output,
            result.size_bytes,
            result.warnings.len(),
            result.warning_counts.convert,
            result.warning_counts.decode,
            result.warning_counts.render,
        );
        for w in &result.warnings {
            match &w.location {
                Some(loc) => eprintln!("[{}] {} ({loc}): {}", w.stage, w.code, w.message),
                None => eprintln!("[{}] {}: {}", w.stage, w.code, w.message),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_format_by_content() {
        assert_eq!(detect_format(&CFB_MAGIC), Some("hwp5"));
        assert_eq!(detect_format(b"PK\x03\x04zipzip"), Some("hwpx"));
        assert_eq!(detect_format(b"not a container"), None);
        assert_eq!(detect_format(b""), None);
    }

    #[test]
    fn convert_warning_dto_maps_every_variant() {
        let cases = [
            (Hwp5Warning::UnsupportedTag { tag_id: 0x5B, offset: 12 }, "UNSUPPORTED_TAG"),
            (Hwp5Warning::SkippedStream { name: "Scripts".into() }, "SKIPPED_STREAM"),
            (
                Hwp5Warning::DroppedControl { control: "ole_object", reason: "x".into() },
                "DROPPED_CONTROL",
            ),
            (
                Hwp5Warning::ProjectionFallback { subject: "s", reason: "r".into() },
                "PROJECTION_FALLBACK",
            ),
            (Hwp5Warning::ParserFallback { subject: "s", reason: "r".into() }, "PARSER_FALLBACK"),
        ];
        for (w, code) in cases {
            let dto = convert_warning_dto(&hwpforge_convert::ConvertWarning::Hwp5(w));
            assert_eq!(dto.stage, "convert");
            assert_eq!(dto.code, code);
        }
    }

    #[test]
    fn decode_warning_dto_carries_attribute_location() {
        let dto = decode_warning_dto(&DecodeWarning::UnknownEnumValue {
            attribute: "hp:header@applyPageType",
            raw: "WEIRD".into(),
            fallback: "BOTH",
        });
        assert_eq!((dto.stage, dto.code.as_str()), ("decode", "UNKNOWN_ENUM_VALUE"));
        assert_eq!(dto.location.as_deref(), Some("hp:header@applyPageType"));
    }

    /// Was `render_warning_dto_maps_every_variant`, pinning the DTO's `code`
    /// against a hardcoded literal per variant — that only proved the CLI's
    /// own (now-removed) private mapping matched itself, not that it agreed
    /// with `ConvertOpsWarning::info()`, the ops crate's actual source of
    /// truth (audit finding). Now that `render_warning_dto` calls `.info()`
    /// directly, this compares against that call instead, so a future
    /// regression that reintroduces a separate mapping — and drifts from it
    /// — fails here rather than passing silently. `location` keeps its
    /// literal expectation: `render_warning_location`'s per-variant match
    /// is hand-written and still worth checking against real values.
    ///
    /// LOW (audit): this list was missing the four image variants
    /// (`ImageDataMissing`/`UnsupportedImageFormat`/`ImageDecodeFailed`/
    /// `InvalidImageGeometry`) and never asserted `message`, only `code` and
    /// `location` — a drift in `.info()`'s message text for any variant
    /// would have passed silently. `variant_name` below is the
    /// exhaustiveness guard: it is a second, independent match over every
    /// `PdfWarning` variant with no wildcard arm reused from production
    /// code, so a variant *this test's own match forgot* panics instead of
    /// reporting fewer than 22 cases. `PdfWarning` is `#[non_exhaustive]`
    /// from this (downstream) crate's point of view, so rustc cannot itself
    /// refuse to compile when the enum gains a variant upstream — the same
    /// trade-off `render_warning_dto`/`render_warning_location`'s own
    /// wildcard arms already accept — but a variant *added to `cases` below
    /// without a matching arm here* fails at runtime rather than at review
    /// time.
    #[test]
    fn render_warning_dto_matches_ops_info_for_every_variant() {
        use hwpforge_smithy_pdf::font::FaceStyle;
        let loc = || "s0/p1/l2".to_string();
        let cases: Vec<(PdfWarning, Option<&str>)> = vec![
            (PdfWarning::ParagraphSkipped { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::PageEventLost { location: loc() }, Some("s0/p1/l2")),
            (
                PdfWarning::FontStyleFallback {
                    face: "f".into(),
                    requested: FaceStyle::Bold,
                    location: loc(),
                },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::ImageDataMissing { key: "img1".into(), location: loc() },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::UnsupportedImageFormat {
                    key: "img1".into(),
                    format: "bmp",
                    location: loc(),
                },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::ImageDecodeFailed {
                    key: "img1".into(),
                    detail: "bad header".into(),
                    location: loc(),
                },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::InvalidImageGeometry {
                    key: "img1".into(),
                    detail: "zero width".into(),
                    location: loc(),
                },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::FontAxisFallback { fonts: vec!["a".into()], location: loc() },
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::FontEmbedPreviewPrint {
                    face: "f".into(),
                    path: "/x".into(),
                    fingerprint: "00".into(),
                },
                None,
            ),
            (PdfWarning::AlignmentApproximated { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::AnchorMarkerOnLineBoundary { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::NonTextRunDropped { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::TablePaginationComputed { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::TableDeficitDistributed { location: loc() }, Some("s0/p1/l2")),
            (
                PdfWarning::UnsupportedTableStyle { location: loc(), what: "cell fill" },
                Some("s0/p1/l2"),
            ),
            (PdfWarning::BandOverflow { kind: "header", location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::PageStartsOnFallback { section: 0 }, Some("s0")),
            (PdfWarning::VertAlignFallback { location: loc() }, Some("s0/p1/l2")),
            (PdfWarning::PageNumberSkipped { section: 0, what: "position" }, Some("s0")),
            (PdfWarning::PageNumberStyleFallback { section: 0 }, Some("s0")),
            (
                PdfWarning::MissingGlyphs { face: "f".into(), count: 2, location: loc() },
                Some("s0/p1/l2"),
            ),
            (PdfWarning::LineOverflow { location: loc(), excess: 190 }, Some("s0/p1/l2")),
        ];

        /// Names every current `PdfWarning` variant — kept in this test only
        /// (not reused from production code) so it fails on a variant
        /// `cases` above forgot instead of quietly agreeing with whatever
        /// `render_warning_dto`/`render_warning_location` already handle.
        fn variant_name(w: &PdfWarning) -> &'static str {
            match w {
                PdfWarning::ParagraphSkipped { .. } => "ParagraphSkipped",
                PdfWarning::PageEventLost { .. } => "PageEventLost",
                PdfWarning::FontStyleFallback { .. } => "FontStyleFallback",
                PdfWarning::ImageDataMissing { .. } => "ImageDataMissing",
                PdfWarning::UnsupportedImageFormat { .. } => "UnsupportedImageFormat",
                PdfWarning::ImageDecodeFailed { .. } => "ImageDecodeFailed",
                PdfWarning::InvalidImageGeometry { .. } => "InvalidImageGeometry",
                PdfWarning::FontAxisFallback { .. } => "FontAxisFallback",
                PdfWarning::FontEmbedPreviewPrint { .. } => "FontEmbedPreviewPrint",
                PdfWarning::AlignmentApproximated { .. } => "AlignmentApproximated",
                PdfWarning::NonTextRunDropped { .. } => "NonTextRunDropped",
                PdfWarning::AnchorMarkerOnLineBoundary { .. } => "AnchorMarkerOnLineBoundary",
                PdfWarning::TablePaginationComputed { .. } => "TablePaginationComputed",
                PdfWarning::TableDeficitDistributed { .. } => "TableDeficitDistributed",
                PdfWarning::UnsupportedTableStyle { .. } => "UnsupportedTableStyle",
                PdfWarning::BandOverflow { .. } => "BandOverflow",
                PdfWarning::PageStartsOnFallback { .. } => "PageStartsOnFallback",
                PdfWarning::VertAlignFallback { .. } => "VertAlignFallback",
                PdfWarning::PageNumberSkipped { .. } => "PageNumberSkipped",
                PdfWarning::PageNumberStyleFallback { .. } => "PageNumberStyleFallback",
                PdfWarning::MissingGlyphs { .. } => "MissingGlyphs",
                PdfWarning::LineOverflow { .. } => "LineOverflow",
                // Forced by `#[non_exhaustive]`, not a real "don't care": a
                // variant reaching this arm is one `cases` above has not
                // been taught about yet.
                other => {
                    panic!("PdfWarning variant not covered by this test's case list: {other:?}")
                }
            }
        }

        let mut seen: Vec<&'static str> = cases.iter().map(|(w, _)| variant_name(w)).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 22, "expected exactly 22 distinct PdfWarning variants in `cases`");

        for (w, expected_location) in &cases {
            let dto = render_warning_dto(w);
            let info = ConvertOpsWarning::Render(w.clone()).info();
            assert_eq!(dto.stage, "render");
            assert_eq!(dto.code, info.code, "{w:?}");
            assert_eq!(dto.message, info.message, "{w:?}");
            assert_eq!(dto.location.as_deref(), *expected_location, "{w:?}");
        }
    }

    // `pdf_error_cause_maps_every_variant` is gone along with `pdf_error_cause`
    // (W3): the `PDF_RENDER_FAILED` cause now comes from
    // `compat::convert_error`, which delegates to
    // `hwpforge_convert::ops::ConvertOpsError::cause_info` — that mapping's
    // own doc comment says it "reproduces the CLI's `pdf_error_cause`", and
    // `hwpforge-convert`'s `tests/ops_inventory.rs` already proves it stays
    // exhaustive over `PdfErrorCode` as the enum grows. Re-testing the same
    // table here would just duplicate that coverage in a crate that no
    // longer owns the mapping.

    #[test]
    fn parse_discovery_accepts_documented_modes() {
        assert!(matches!(parse_discovery("explicit", false), FontDiscovery::ExplicitOnly));
        assert!(matches!(parse_discovery("hancom", false), FontDiscovery::HancomBundle));
        assert!(matches!(parse_discovery("platform", false), FontDiscovery::Platform));
    }
}
