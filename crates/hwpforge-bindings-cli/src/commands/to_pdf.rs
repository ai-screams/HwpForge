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
use hwpforge_smithy_pdf::font::FontDiscovery;

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

/// 경고 DTO 한 줄 — 네 열 **전부** `hwpforge-convert` 에서 온다:
/// `stage` 는 [`ConvertOpsWarning::stage`], 나머지 셋은
/// [`ConvertOpsWarning::parts`]. CLI 는 자체 매핑을 갖지 않는다.
///
/// 예전에는 단계마다 손 매핑이 하나씩(convert·decode·render) 있었고 그중
/// render 만 [`ConvertOpsWarning::info`] 로 옮겨 갔다 — `info()` 의 message
/// 는 location 을 접두한 형태라, render 경고만 같은 값이 `message` 와
/// `location` 두 열에 실리고 convert·decode 는 한 번만 실리는 불일치가
/// 한 배열 안에 생겼다(감사 지적 C1). `parts()` 는 접두 전 세 열이므로
/// 세 단계가 다시 같은 규칙을 따른다.
///
/// `input` 단계만 예외로 `run` 이 직접 만든다: `EXTENSION_MISMATCH` 는
/// 파일명과 스니핑 결과를 비교한 것이고, ops 는 바이트만 받아 파일명을
/// 보지 못한다.
fn warning_dto(w: &ConvertOpsWarning) -> WarningDto {
    let parts = w.parts();
    WarningDto {
        stage: w.stage(),
        code: parts.code,
        message: parts.message,
        location: parts.location,
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
    let rendered = to_pdf(&bytes, &pdf_options)
        .unwrap_or_else(|err| compat::exit_convert_error(Command::ToPdf, err, json_mode));
    // 단계별 분기가 없다 — `stage()`/`parts()` 가 변형마다 답을 안다.
    // CLI 가 `match` 를 하면 `ConvertOpsWarning` 이 `#[non_exhaustive]` 라
    // 하류에서 `_` arm 이 강제되고, 그 arm 은 미래의 네 번째 변형에 단계를
    // 임의로(`"render"`) 붙이게 된다 — C1 과 같은 종류의 조용한 오표기.
    warnings.extend(rendered.warnings.iter().map(warning_dto));

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
    use hwpforge_convert::ConvertWarning;
    use hwpforge_smithy_hwp5::Hwp5Warning;
    use hwpforge_smithy_hwpx::{DecodeWarning, EncodeWarning, ParagraphPath, PathSeg};
    use hwpforge_smithy_pdf::font::FaceStyle;
    use hwpforge_smithy_pdf::PdfWarning;

    use super::*;

    #[test]
    fn detect_format_by_content() {
        assert_eq!(detect_format(&CFB_MAGIC), Some("hwp5"));
        assert_eq!(detect_format(b"PK\x03\x04zipzip"), Some("hwpx"));
        assert_eq!(detect_format(b"not a container"), None);
        assert_eq!(detect_format(b""), None);
    }

    fn paragraph_path() -> ParagraphPath {
        ParagraphPath(vec![PathSeg::Section(0)])
    }

    /// C1 회귀 잠금: `location` 열이 있는 DTO 의 `message` 가 그 값을 다시
    /// 접두하지 않는다. `':'` 유무로 보지 않는 이유 — `DROPPED_CONTROL` ·
    /// `PROJECTION_FALLBACK` · `PARSER_FALLBACK` 의 message 는 `location`
    /// 이 없는데도 `"{subject}: "` 로 시작한다.
    fn assert_no_location_prefix(dto: &WarningDto) {
        if let Some(location) = &dto.location {
            assert!(
                !dto.message.starts_with(&format!("{location}: ")),
                "[{}] {} 의 message 가 location 을 되풀이한다: {}",
                dto.stage,
                dto.code,
                dto.message,
            );
        }
    }

    /// 세 열(`code`/`message`/`location`)이 [`ConvertOpsWarning::info`] 한
    /// 줄을 그대로 복원함 — 두 표면이 갈라지면 여기서 잡힌다. `info()` 의
    /// 접기 규칙(`"{location}: {message}"`)은 ops 크레이트 안의 private
    /// 헬퍼라, 소비자 쪽에서 관례를 직접 적어 못박는다.
    fn assert_dto_folds_into_info(warning: &ConvertOpsWarning, dto: &WarningDto) {
        let info = warning.info();
        assert_eq!(info.code, dto.code, "{warning:?}");
        let folded = match &dto.location {
            Some(location) => format!("{location}: {}", dto.message),
            None => dto.message.clone(),
        };
        assert_eq!(info.message, folded, "{warning:?}");
    }

    /// W5(`6ef353b`)의 `convert_warning_dto` match 가 내던 `(code, message,
    /// location)` 을 리터럴로 고정한다. 이전 판은 `code` 5 개만 봤고
    /// `message` 는 보지 않았다.
    #[test]
    fn convert_stage_dto_maps_every_variant() {
        let cases: Vec<(ConvertWarning, &str, &str, Option<&str>)> = vec![
            (
                ConvertWarning::Hwp5(Hwp5Warning::UnsupportedTag { tag_id: 0x5B, offset: 12 }),
                "UNSUPPORTED_TAG",
                "unsupported record tag 0x5B",
                Some("offset 12"),
            ),
            (
                ConvertWarning::Hwp5(Hwp5Warning::SkippedStream { name: "Scripts".into() }),
                "SKIPPED_STREAM",
                "stream skipped: Scripts",
                None,
            ),
            (
                ConvertWarning::Hwp5(Hwp5Warning::DroppedControl {
                    control: "ole_object",
                    reason: "x".into(),
                }),
                "DROPPED_CONTROL",
                "ole_object: x",
                None,
            ),
            (
                ConvertWarning::Hwp5(Hwp5Warning::ProjectionFallback {
                    subject: "s",
                    reason: "r".into(),
                }),
                "PROJECTION_FALLBACK",
                "s: r",
                None,
            ),
            (
                ConvertWarning::Hwp5(Hwp5Warning::ParserFallback {
                    subject: "s",
                    reason: "r".into(),
                }),
                "PARSER_FALLBACK",
                "s: r",
                None,
            ),
            (
                ConvertWarning::Hwp5(Hwp5Warning::LayoutCacheDropped { reason: "ledger".into() }),
                "LAYOUT_CACHE_DROPPED",
                "ledger",
                None,
            ),
            (
                ConvertWarning::HwpxEncode(EncodeWarning::LayoutCacheDropped {
                    path: paragraph_path(),
                    reason: "ledger".into(),
                }),
                "LAYOUT_CACHE_DROPPED",
                "ledger",
                Some("section[0]"),
            ),
        ];
        for (warning, code, message, location) in &cases {
            let wrapped = ConvertOpsWarning::Convert(warning.clone());
            let dto = warning_dto(&wrapped);
            assert_eq!(dto.stage, "convert", "{warning:?}");
            assert_eq!(dto.code, *code, "{warning:?}");
            assert_eq!(dto.message, *message, "{warning:?}");
            assert_eq!(dto.location.as_deref(), *location, "{warning:?}");
            assert_no_location_prefix(&dto);
            assert_dto_folds_into_info(&wrapped, &dto);
        }

        // 분류되지 않은 인코드 경고는 `OTHER` 로 떨어지되 텍스트를 잃지 않는다.
        let unclassified = ConvertWarning::HwpxEncode(EncodeWarning::NoteHeadSkipped {
            path: paragraph_path(),
            reason: "titleMark".into(),
        });
        let dto = warning_dto(&ConvertOpsWarning::Convert(unclassified));
        assert_eq!(dto.stage, "convert");
        assert_eq!(dto.code, "OTHER");
        assert!(dto.message.contains("NoteHeadSkipped"), "{}", dto.message);
        assert!(dto.message.contains("titleMark"), "{}", dto.message);
        assert_eq!(dto.location, None);
    }

    /// W5 의 `decode_warning_dto` 문구 고정 — 속성 경로는 `location` 열에만
    /// 실리고 message 에는 섞이지 않는다.
    #[test]
    fn decode_stage_dto_carries_attribute_location() {
        let warning = DecodeWarning::UnknownEnumValue {
            attribute: "hp:header@applyPageType",
            raw: "WEIRD".into(),
            fallback: "BOTH",
        };
        let wrapped = ConvertOpsWarning::Decode(warning);
        let dto = warning_dto(&wrapped);
        assert_eq!(dto.stage, "decode");
        assert_eq!(dto.code, "UNKNOWN_ENUM_VALUE");
        assert_eq!(dto.message, "\"WEIRD\" unknown — fell back to BOTH");
        assert_eq!(dto.location.as_deref(), Some("hp:header@applyPageType"));
        assert_no_location_prefix(&dto);
        assert_dto_folds_into_info(&wrapped, &dto);

        // 두 번째 디코드 변형은 전용 문구가 없어 `OTHER` 로 간다.
        let dropped =
            DecodeWarning::LayoutCacheDropped { path: paragraph_path(), reason: "textpos".into() };
        let dto = warning_dto(&ConvertOpsWarning::Decode(dropped));
        assert_eq!(dto.stage, "decode");
        assert_eq!(dto.code, "OTHER");
        assert!(dto.message.contains("LayoutCacheDropped"), "{}", dto.message);
        assert_eq!(dto.location, None);
    }

    /// W5(`6ef353b`)의 `render_warning_dto` match(같은 파일 133–249 행)가
    /// 내던 `(code, message, location)` 세 열을 **리터럴로** 고정한다.
    ///
    /// 이전 판은 `assert_eq!(dto.message, info.message)` 였다 — DTO 가
    /// `info()` 를 그대로 옮겨 담았으므로 자기 자신과 비교하는 토톨로지였고,
    /// 그 사이 `info()` 가 location 을 message 에 접두하도록 바뀌면서 CLI 의
    /// `message` 열에 접두 문구가 들어오고 `location` 열에도 같은 값이
    /// 실리는 이중 표기가 생겼는데도 초록이었다(감사 지적 C1). 이제 기대값은
    /// W5 문구이므로 그 회귀가 여기서 실패한다.
    ///
    /// `variant_name` 은 망라 가드다: 프로덕션 코드에서 재사용하지 않은
    /// 독립 match 라, `cases` 가 빠뜨린 변형은 22 미만으로 조용히 보고되는
    /// 대신 panic 한다. `PdfWarning` 은 이 (하류) 크레이트에서
    /// `#[non_exhaustive]` 라 상류에 변형이 늘어도 rustc 가 컴파일을 막지
    /// 못하므로 — 런타임 가드가 그 자리를 대신한다.
    #[test]
    fn render_stage_dto_pins_the_w5_wording_for_every_variant() {
        let loc = || "s0/p1/l2".to_string();
        let cases: Vec<(PdfWarning, &str, &str, Option<&str>)> = vec![
            (
                PdfWarning::ParagraphSkipped { location: loc() },
                "PARAGRAPH_SKIPPED",
                "paragraph without layout cache skipped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::PageEventLost { location: loc() },
                "PAGE_EVENT_LOST",
                "page-number restart/hiding control on a cacheless paragraph — event lost",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::FontStyleFallback {
                    face: "f".into(),
                    requested: FaceStyle::Bold,
                    location: loc(),
                },
                "FONT_STYLE_FALLBACK",
                "\"f\" has no Bold face — rendered regular",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::ImageDataMissing { key: "img1".into(), location: loc() },
                "IMAGE_DATA_MISSING",
                "image data missing for \"img1\" — skipped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::UnsupportedImageFormat {
                    key: "img1".into(),
                    format: "bmp",
                    location: loc(),
                },
                "UNSUPPORTED_IMAGE_FORMAT",
                "\"img1\" is bmp — not renderable, skipped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::ImageDecodeFailed {
                    key: "img1".into(),
                    detail: "bad header".into(),
                    location: loc(),
                },
                "IMAGE_DECODE_FAILED",
                "\"img1\": bad header — skipped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::InvalidImageGeometry {
                    key: "img1".into(),
                    detail: "zero width".into(),
                    location: loc(),
                },
                "INVALID_IMAGE_GEOMETRY",
                "\"img1\": zero width — skipped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::FontAxisFallback { fonts: vec!["a".into()], location: loc() },
                "FONT_AXIS_FALLBACK",
                "per-language axis fonts [\"a\"] — rendered with hangul axis",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::FontEmbedPreviewPrint {
                    face: "f".into(),
                    path: "/x".into(),
                    fingerprint: "00".into(),
                },
                "FONT_EMBED_PREVIEW_PRINT",
                "\"f\" (/x) is Preview & Print licensed",
                None,
            ),
            (
                PdfWarning::AlignmentApproximated { location: loc() },
                "ALIGNMENT_APPROXIMATED",
                "distributed alignment approximated",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::AnchorMarkerOnLineBoundary { location: loc() },
                "ANCHOR_MARKER_ON_LINE_BOUNDARY",
                "anchored-image marker sits exactly on a line boundary — \
                 anchored to the later line",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::NonTextRunDropped { location: loc() },
                "NON_TEXT_RUN_DROPPED",
                "non-text run (control/image) dropped",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::TablePaginationComputed { location: loc() },
                "TABLE_PAGINATION_COMPUTED",
                "split-table page boundary computed (cache has no signal)",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::TableDeficitDistributed { location: loc() },
                "TABLE_DEFICIT_DISTRIBUTED",
                "merged-cell height deficit redistributed",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::UnsupportedTableStyle { location: loc(), what: "cell fill" },
                "UNSUPPORTED_TABLE_STYLE",
                "unsupported table style dropped: cell fill",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::BandOverflow { kind: "header", location: loc() },
                "BAND_OVERFLOW",
                "header exceeds its band — replayed unclipped (Hancom behavior)",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::PageStartsOnFallback { section: 0 },
                "PAGE_STARTS_ON_FALLBACK",
                "pageStartsOn != BOTH is unmeasured — rendered as BOTH",
                Some("s0"),
            ),
            (
                PdfWarning::VertAlignFallback { location: loc() },
                "VERT_ALIGN_FALLBACK",
                "header/footer vertAlign != TOP is unmeasured — rendered as TOP",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::PageNumberSkipped { section: 0, what: "position" },
                "PAGE_NUMBER_SKIPPED",
                "page number skipped — unmeasured position",
                Some("s0"),
            ),
            (
                PdfWarning::PageNumberStyleFallback { section: 0 },
                "PAGE_NUMBER_STYLE_FALLBACK",
                "\"쪽 번호\" CHAR style absent — fell back to default char shape",
                Some("s0"),
            ),
            (
                PdfWarning::MissingGlyphs { face: "f".into(), count: 2, location: loc() },
                "MISSING_GLYPHS",
                "\"f\" lacks glyphs for 2 character(s) — rendered as tofu",
                Some("s0/p1/l2"),
            ),
            (
                PdfWarning::LineOverflow { location: loc(), excess: 190 },
                "LINE_OVERFLOW",
                "line exceeds its cached box by 190 HWPUNIT (char spacing/scale not carried)",
                Some("s0/p1/l2"),
            ),
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

        let mut seen: Vec<&'static str> = cases.iter().map(|(w, ..)| variant_name(w)).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 22, "expected exactly 22 distinct PdfWarning variants in `cases`");

        for (warning, code, message, location) in &cases {
            let wrapped = ConvertOpsWarning::Render(warning.clone());
            let dto = warning_dto(&wrapped);
            assert_eq!(dto.stage, "render", "{warning:?}");
            assert_eq!(dto.code, *code, "{warning:?}");
            assert_eq!(dto.message, *message, "{warning:?}");
            assert_eq!(dto.location.as_deref(), *location, "{warning:?}");
            assert_no_location_prefix(&dto);
            assert_dto_folds_into_info(&wrapped, &dto);
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
