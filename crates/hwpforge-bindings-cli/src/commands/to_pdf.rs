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
                code: "LAYOUT_CACHE_DROPPED",
                message: reason.clone(),
                location: Some(path.to_string()),
            };
        }
        other => {
            return WarningDto {
                stage: "convert",
                code: "OTHER",
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
        PdfWarning::PageEventLost { location } => (
            "PAGE_EVENT_LOST",
            "page-number restart/hiding control on a cacheless paragraph — event lost".to_string(),
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
        PdfWarning::AnchorMarkerOnLineBoundary { location } => (
            "ANCHOR_MARKER_ON_LINE_BOUNDARY",
            "anchored-image marker sits exactly on a line boundary — \
             anchored to the later line"
                .to_string(),
            Some(location.clone()),
        ),
        PdfWarning::ImageDataMissing { key, location } => (
            "IMAGE_DATA_MISSING",
            format!("image data missing for \"{key}\" — skipped"),
            Some(location.clone()),
        ),
        PdfWarning::UnsupportedImageFormat { key, format, location } => (
            "UNSUPPORTED_IMAGE_FORMAT",
            format!("\"{key}\" is {format} — not renderable, skipped"),
            Some(location.clone()),
        ),
        PdfWarning::ImageDecodeFailed { key, detail, location } => (
            "IMAGE_DECODE_FAILED",
            format!("\"{key}\": {detail} — skipped"),
            Some(location.clone()),
        ),
        PdfWarning::InvalidImageGeometry { key, detail, location } => (
            "INVALID_IMAGE_GEOMETRY",
            format!("\"{key}\": {detail} — skipped"),
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
                code: "OTHER",
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
        assert_eq!((dto.stage, dto.code), ("decode", "UNKNOWN_ENUM_VALUE"));
        assert_eq!(dto.location.as_deref(), Some("hp:header@applyPageType"));
    }

    #[test]
    fn render_warning_dto_maps_every_variant() {
        use hwpforge_smithy_pdf::font::FaceStyle;
        let loc = || "s0/p1/l2".to_string();
        let cases: Vec<(PdfWarning, &str)> = vec![
            (PdfWarning::ParagraphSkipped { location: loc() }, "PARAGRAPH_SKIPPED"),
            (PdfWarning::PageEventLost { location: loc() }, "PAGE_EVENT_LOST"),
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
            (
                PdfWarning::AnchorMarkerOnLineBoundary { location: loc() },
                "ANCHOR_MARKER_ON_LINE_BOUNDARY",
            ),
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
