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

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_options, ConvertOptions};
use hwpforge_smithy_hwp5::Hwp5Warning;
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder};
use hwpforge_smithy_pdf::font::FontDiscovery;
use hwpforge_smithy_pdf::{
    render_document, FontFallbackMode, PartialCachePolicy, PdfInput, PdfOptions, PdfWarning,
};

use crate::error::{check_file_size, CliError};

/// OLE2/CFB magic — HWP5 컨테이너.
const CFB_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

#[derive(Serialize)]
struct WarningDto {
    /// 발생 단계: `input`(디스패치) | `convert`(HWP5→HWPX) | `decode`(HWPX 해석)
    /// | `render`(PDF).
    stage: &'static str,
    /// 안정 코드 (variant 유래 — 스크립트 필터링용).
    code: &'static str,
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

fn convert_warning_dto(w: &Hwp5Warning) -> WarningDto {
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
        other => ("OTHER", format!("{other:?}"), None),
    };
    WarningDto { stage: "convert", code, message, location }
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
    WarningDto { stage: "decode", code, message, location }
}

fn render_warning_dto(w: &PdfWarning) -> WarningDto {
    let (code, message, location) = match w {
        PdfWarning::ParagraphSkipped { location } => (
            "PARAGRAPH_SKIPPED",
            "paragraph without layout cache skipped".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::FontStyleFallback { face, requested, location } => (
            "FONT_STYLE_FALLBACK",
            format!("{face:?} has no {requested:?} face — rendered regular"),
            Some(location.clone()),
        ),
        PdfWarning::FontAxisFallback { fonts, location } => (
            "FONT_AXIS_FALLBACK",
            format!("per-language axis fonts {fonts:?} — rendered with hangul axis"),
            Some(location.clone()),
        ),
        PdfWarning::FontEmbedPreviewPrint { face, path, .. } => (
            "FONT_EMBED_PREVIEW_PRINT",
            format!("{face:?} ({}) is Preview & Print licensed", path.display()),
            None,
        ),
        PdfWarning::AlignmentApproximated { location } => (
            "ALIGNMENT_APPROXIMATED",
            "distributed alignment approximated".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::NonTextRunDropped { location } => (
            "NON_TEXT_RUN_DROPPED",
            "non-text run (control/image) dropped".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::TablePaginationComputed { location } => (
            "TABLE_PAGINATION_COMPUTED",
            "split-table page boundary computed (cache has no signal)".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::TableDeficitDistributed { location } => (
            "TABLE_DEFICIT_DISTRIBUTED",
            "merged-cell height deficit redistributed".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::UnsupportedTableStyle { location, what } => (
            "UNSUPPORTED_TABLE_STYLE",
            format!("unsupported table style dropped: {what}"),
            Some(location.clone()),
        ),
        PdfWarning::BandOverflow { kind, location } => (
            "BAND_OVERFLOW",
            format!("{kind} exceeds its band — replayed unclipped (Hancom behavior)"),
            Some(location.clone()),
        ),
        PdfWarning::PageStartsOnFallback { section } => (
            "PAGE_STARTS_ON_FALLBACK",
            "pageStartsOn != BOTH is unmeasured — rendered as BOTH".to_string(),
            Some(format!("s{section}")),
        ),
        PdfWarning::VertAlignFallback { location } => (
            "VERT_ALIGN_FALLBACK",
            "header/footer vertAlign != TOP is unmeasured — rendered as TOP".to_string(),
            Some(location.clone()),
        ),
        PdfWarning::PageNumberSkipped { section, what } => (
            "PAGE_NUMBER_SKIPPED",
            format!("page number skipped — unmeasured {what}"),
            Some(format!("s{section}")),
        ),
        PdfWarning::PageNumberStyleFallback { section } => (
            "PAGE_NUMBER_STYLE_FALLBACK",
            "\"쪽 번호\" CHAR style absent — fell back to default char shape".to_string(),
            Some(format!("s{section}")),
        ),
        PdfWarning::MissingGlyphs { face, count, location } => (
            "MISSING_GLYPHS",
            format!("{face:?} lacks glyphs for {count} character(s) — rendered as tofu"),
            Some(location.clone()),
        ),
        PdfWarning::LineOverflow { location, excess } => (
            "LINE_OVERFLOW",
            format!(
                "line exceeds its cached box by {excess} HWPUNIT (char spacing/scale not carried)"
            ),
            Some(location.clone()),
        ),
        other => ("OTHER", format!("{other:?}"), None),
    };
    WarningDto { stage: "render", code, message, location }
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
    let bytes = std::fs::read(input).unwrap_or_else(|err| {
        CliError::new("FILE_READ_FAILED", format!("Cannot read '{}': {err}", input.display()))
            .exit(json_mode, 1)
    });

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
                code: "EXTENSION_MISMATCH",
                message: format!("extension implies {implied} but content is {detected}"),
                location: None,
            });
        }
    }

    // HWP5 → HWPX (조판 캐시 carry — 렌더 재료).
    let hwpx_bytes = if detected == "hwp5" {
        let (converted, convert_warnings) = hwp5_to_hwpx_bytes_with_options(
            &bytes,
            ConvertOptions::default().with_carry_layout_cache(true),
        )
        .unwrap_or_else(|err| {
            CliError::new(
                "HWP5_CONVERT_FAILED",
                format!("Cannot convert '{}' to HWPX: {err}", input.display()),
            )
            .exit(json_mode, 2)
        });
        warnings.extend(convert_warnings.iter().map(convert_warning_dto));
        converted
    } else {
        bytes
    };

    let decoded = HwpxDecoder::decode(&hwpx_bytes).unwrap_or_else(|err| {
        CliError::new("HWPX_DECODE_FAILED", format!("Cannot decode '{}': {err}", input.display()))
            .exit(json_mode, 2)
    });
    warnings.extend(decoded.warnings.iter().map(decode_warning_dto));

    let validated = decoded.document.validate().unwrap_or_else(|err| {
        CliError::new("VALIDATION_FAILED", format!("Document validation failed: {err}"))
            .exit(json_mode, 2)
    });

    let mut options = PdfOptions::default();
    options.font_dirs = font_dirs.to_vec();
    options.discovery = discovery;
    options.font_fallback =
        if degraded { FontFallbackMode::Degraded } else { FontFallbackMode::Fatal };
    options.partial_cache = if partial_cache_reject {
        PartialCachePolicy::Reject
    } else {
        PartialCachePolicy::WarnAndSkip
    };

    let rendered =
        render_document(&PdfInput { document: &validated, styles: &decoded.style_store }, &options)
            .unwrap_or_else(|err| {
                CliError::new("PDF_RENDER_FAILED", format!("Cannot render PDF: {err}"))
                    .with_hint(
                        "cacheless documents need a Hancom re-save; font errors may need \
                         --font-dir/--discovery or --degraded",
                    )
                    .exit(json_mode, 2)
            });
    warnings.extend(rendered.warnings.iter().map(render_warning_dto));

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
            let dto = convert_warning_dto(&w);
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
        assert_eq!((dto.stage, dto.code), ("decode", "UNKNOWN_ENUM_VALUE"));
        assert_eq!(dto.location.as_deref(), Some("hp:header@applyPageType"));
    }

    #[test]
    fn render_warning_dto_maps_every_variant() {
        use hwpforge_smithy_pdf::font::FaceStyle;
        let loc = || "s0/p1/l2".to_string();
        let cases: Vec<(PdfWarning, &str)> = vec![
            (PdfWarning::ParagraphSkipped { location: loc() }, "PARAGRAPH_SKIPPED"),
            (
                PdfWarning::FontStyleFallback {
                    face: "f".into(),
                    requested: FaceStyle::Bold,
                    location: loc(),
                },
                "FONT_STYLE_FALLBACK",
            ),
            (
                PdfWarning::FontAxisFallback { fonts: vec!["a".into()], location: loc() },
                "FONT_AXIS_FALLBACK",
            ),
            (
                PdfWarning::FontEmbedPreviewPrint {
                    face: "f".into(),
                    path: "/x".into(),
                    fingerprint: "00".into(),
                },
                "FONT_EMBED_PREVIEW_PRINT",
            ),
            (PdfWarning::AlignmentApproximated { location: loc() }, "ALIGNMENT_APPROXIMATED"),
            (PdfWarning::NonTextRunDropped { location: loc() }, "NON_TEXT_RUN_DROPPED"),
            (PdfWarning::TablePaginationComputed { location: loc() }, "TABLE_PAGINATION_COMPUTED"),
            (PdfWarning::TableDeficitDistributed { location: loc() }, "TABLE_DEFICIT_DISTRIBUTED"),
            (
                PdfWarning::UnsupportedTableStyle { location: loc(), what: "cell fill" },
                "UNSUPPORTED_TABLE_STYLE",
            ),
            (PdfWarning::BandOverflow { kind: "header", location: loc() }, "BAND_OVERFLOW"),
            (PdfWarning::PageStartsOnFallback { section: 0 }, "PAGE_STARTS_ON_FALLBACK"),
            (PdfWarning::VertAlignFallback { location: loc() }, "VERT_ALIGN_FALLBACK"),
            (PdfWarning::PageNumberSkipped { section: 0, what: "position" }, "PAGE_NUMBER_SKIPPED"),
            (PdfWarning::PageNumberStyleFallback { section: 0 }, "PAGE_NUMBER_STYLE_FALLBACK"),
            (
                PdfWarning::MissingGlyphs { face: "f".into(), count: 2, location: loc() },
                "MISSING_GLYPHS",
            ),
            (PdfWarning::LineOverflow { location: loc(), excess: 190 }, "LINE_OVERFLOW"),
        ];
        for (w, code) in &cases {
            let dto = render_warning_dto(w);
            assert_eq!(dto.stage, "render");
            assert_eq!(&dto.code, code, "{w:?}");
        }
    }

    #[test]
    fn parse_discovery_accepts_documented_modes() {
        assert!(matches!(parse_discovery("explicit", false), FontDiscovery::ExplicitOnly));
        assert!(matches!(parse_discovery("hancom", false), FontDiscovery::HancomBundle));
        assert!(matches!(parse_discovery("platform", false), FontDiscovery::Platform));
    }
}
