//! W2b c4 — tracked PNG/JPEG e2e + 한컴 PDF runtime 대조 (§4 D3/D4).
//!
//! 두 fixture(`inline_treat_as_char_png_body`, `inline_treat_as_char_jpeg_edges`)
//! 를 두 경로로 렌더한다: ① `.hwpx` 직접 렌더, ② `.hwp` → HWPX 변환
//! (`carry_layout_cache=true`) → 렌더. 각 산출물을 `support::extract_pages`
//! 로 읽어 이미지 bbox 를 뽑고, 문서 자체의 margin+layout_cache 로부터
//! 독립 계산한 기대값과 대조한다 (자체 일관성 게이트). 마지막으로 committed
//! 한컴 PDF 를 같은 추출기로 읽어 크기를 대조한다.
//!
//! fixture(`tests/fixtures/images/`) + 한컴 폰트 번들이 모두 있는 머신
//! 에서만 실행된다 (render_pdf.rs 와 동일한 fixture-optional 관례).

mod support;

use std::path::PathBuf;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::layout::LineSeg;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_pdf::{render_document, PdfInput, PdfOptions};

const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/images")
}

fn fixture_path(name: &str) -> PathBuf {
    fixture_dir().join(name)
}

fn font_options() -> Option<PdfOptions> {
    let dir = PathBuf::from(HANCOM_TTF_DIR);
    if !dir.exists() {
        return None;
    }
    let mut options = PdfOptions::default();
    options.font_dirs = vec![dir];
    Some(options)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// oracle.json 을 로드하고, `base` fixture 의 hwp/hwpx/pdf sha256 이 실물
/// 파일과 일치하는지 검증한다 — 실패하면 fixture drift 를 즉시 특정한다.
fn verify_oracle_and_load(base: &str) -> serde_json::Value {
    let oracle_path = fixture_path("inline_treat_as_char_oracle.json");
    let Ok(oracle_bytes) = std::fs::read(&oracle_path) else {
        panic!("oracle.json missing: {oracle_path:?}");
    };
    let oracle: serde_json::Value =
        serde_json::from_slice(&oracle_bytes).expect("oracle.json should be valid JSON");
    let entry = &oracle["fixtures"][base];
    assert!(!entry.is_null(), "oracle.json missing fixture entry for {base}");

    for (ext, key) in [("hwp", "hwp_sha256"), ("hwpx", "hwpx_sha256"), ("pdf", "pdf_sha256")] {
        let expected = entry[key].as_str().unwrap_or_else(|| panic!("oracle missing {key}"));
        let bytes = std::fs::read(fixture_path(&format!("{base}.{ext}")))
            .unwrap_or_else(|e| panic!("{base}.{ext} unreadable: {e}"));
        let actual = sha256_hex(&bytes);
        assert_eq!(actual, expected, "{base}.{ext} sha256 drifted from oracle.json");
    }
    entry.clone()
}

// ── 기대값 계산 (문서 자체의 margin + layout_cache 로부터 독립 산출) ──

struct ExpectedImage {
    width_hwpunit: i32,
    height_hwpunit: i32,
    /// 절대(top-left, HWPUNIT) 호스트 줄 top y.
    host_top_y: i32,
    /// 절대(HWPUNIT) 호스트 줄 좌변/우변.
    host_left_x: i32,
    host_right_x: i32,
    /// 이 이미지가 속한 문단에 텍스트 run 이 전혀 없는지(image-only 문단).
    para_is_image_only: bool,
}

fn host_line_for_offset(lines: &[LineSeg], offset: u32, para_len: u32) -> &LineSeg {
    for (i, line) in lines.iter().enumerate() {
        let start = line.textpos;
        let is_last = i + 1 == lines.len();
        let end = lines.get(i + 1).map_or(para_len, |n| n.textpos);
        let in_range = if is_last {
            offset >= start && offset <= end
        } else {
            offset >= start && offset < end
        };
        if in_range {
            return line;
        }
    }
    lines.last().expect("non-empty layout cache")
}

/// body 문단들을 문서 순서대로 훑어 admitted(treat_as_char) 이미지의
/// 기대 배치를 계산한다 — PDF 렌더 파이프라인과 독립적으로, Core 문서
/// 자신의 margin+layout_cache 데이터만으로 계산한다 (자체 일관성).
fn expected_body_images(doc: &Document<Validated>) -> Vec<ExpectedImage> {
    let mut out = Vec::new();
    for section in doc.sections() {
        let body_top = section.page_settings.margin_top.as_i32()
            + section.page_settings.header_margin.as_i32();
        let body_left = section.page_settings.margin_left.as_i32();
        for para in &section.paragraphs {
            let Some(cache) = para.layout_cache.as_ref() else { continue };
            if cache.lines.is_empty() {
                continue;
            }
            let total_len: u32 = para
                .runs
                .iter()
                .filter_map(|r| r.content.plain_text())
                .map(|t| t.encode_utf16().count() as u32)
                .sum();
            let text_run_count =
                para.runs.iter().filter(|r| r.content.plain_text().is_some()).count();
            let mut cursor: u32 = 0;
            for run in &para.runs {
                if let Some(image) = run.content.as_image() {
                    let treat_as_char = image.placement.as_ref().is_some_and(|p| p.treat_as_char);
                    if treat_as_char {
                        let line = host_line_for_offset(&cache.lines, cursor, total_len);
                        out.push(ExpectedImage {
                            width_hwpunit: image.width.as_i32(),
                            height_hwpunit: image.height.as_i32(),
                            host_top_y: body_top + line.vertpos,
                            host_left_x: body_left + line.horzpos,
                            host_right_x: body_left + line.horzpos + line.horzsize,
                            para_is_image_only: text_run_count == 0,
                        });
                    }
                } else if let Some(text) = run.content.plain_text() {
                    cursor += text.encode_utf16().count() as u32;
                }
            }
        }
    }
    out
}

fn hwpunit_to_pt(v: i32) -> f64 {
    f64::from(v) / 100.0
}

// ── 렌더 경로 헬퍼 ─────────────────────────────────────────────────

fn render_hwpx_bytes(
    hwpx_bytes: &[u8],
    options: &PdfOptions,
) -> (hwpforge_smithy_pdf::PdfOutput, Document<Validated>) {
    let decoded = HwpxDecoder::decode(hwpx_bytes).expect("hwpx decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let input = PdfInput { document: &validated, styles: &lookup };
    let output = render_document(&input, options).expect("render");
    (output, validated)
}

/// 두 경로(① 직접 hwpx, ② hwp convert carry)의 (렌더 산출물, 검증된
/// 문서) 쌍을 반환한다.
fn render_both_paths(
    base: &str,
    options: &PdfOptions,
) -> [(&'static str, hwpforge_smithy_pdf::PdfOutput, Document<Validated>); 2] {
    let hwpx_bytes = std::fs::read(fixture_path(&format!("{base}.hwpx"))).expect("hwpx readable");
    let (direct_output, direct_doc) = render_hwpx_bytes(&hwpx_bytes, options);

    let hwp_bytes = std::fs::read(fixture_path(&format!("{base}.hwp"))).expect("hwp readable");
    let convert_options = hwpforge_convert::ConvertOptions::default().with_carry_layout_cache(true);
    let (carried_hwpx_bytes, convert_warnings) =
        hwpforge_convert::hwp5_to_hwpx_bytes_with_options(&hwp_bytes, convert_options)
            .expect("hwp5 -> hwpx carry conversion");
    assert!(convert_warnings.is_empty(), "{base}: carry conversion warnings: {convert_warnings:?}");
    let (carried_output, carried_doc) = render_hwpx_bytes(&carried_hwpx_bytes, options);

    [("direct-hwpx", direct_output, direct_doc), ("hwp-carry", carried_output, carried_doc)]
}

/// 한 경로의 렌더 산출물에 대해 자체 일관성 게이트를 검사한다.
fn assert_self_consistency(
    base: &str,
    path_name: &str,
    output: &hwpforge_smithy_pdf::PdfOutput,
    doc: &Document<Validated>,
) {
    assert!(output.bytes.starts_with(b"%PDF-"), "{base}/{path_name}: PDF 헤더");
    assert!(
        output.warnings.is_empty(),
        "{base}/{path_name}: 경고 0 이어야 함 — {:?}",
        output.warnings
    );

    let expected = expected_body_images(doc);
    let pages = support::extract_pages(&output.bytes);
    assert!(!pages.is_empty(), "{base}/{path_name}: 최소 1쪽");
    // MediaBox 가 문서 자신의 PageSettings 와 일치하는지 — 추출기의 페이지
    // 크기 파싱 자체를 문서 데이터로 잠근다.
    let section0 = &doc.sections()[0];
    assert!(
        support::approx_eq(
            pages[0].width,
            hwpunit_to_pt(section0.page_settings.width.as_i32()),
            0.01
        ),
        "{base}/{path_name}: MediaBox width got={} expected={}",
        pages[0].width,
        hwpunit_to_pt(section0.page_settings.width.as_i32())
    );
    assert!(
        support::approx_eq(
            pages[0].height,
            hwpunit_to_pt(section0.page_settings.height.as_i32()),
            0.01
        ),
        "{base}/{path_name}: MediaBox height got={} expected={}",
        pages[0].height,
        hwpunit_to_pt(section0.page_settings.height.as_i32())
    );
    let extracted = &pages[0].images;
    assert_eq!(
        extracted.len(),
        expected.len(),
        "{base}/{path_name}: 추출된 이미지 개수가 기대와 달라야 함 — extracted={extracted:?}"
    );

    for (idx, (exp, got)) in expected.iter().zip(extracted.iter()).enumerate() {
        let expected_w = hwpunit_to_pt(exp.width_hwpunit);
        let expected_h = hwpunit_to_pt(exp.height_hwpunit);
        assert!(
            support::approx_eq(got.width, expected_w, 0.01),
            "{base}/{path_name} image[{idx}]: width got={} expected={expected_w}",
            got.width
        );
        assert!(
            support::approx_eq(got.height, expected_h, 0.01),
            "{base}/{path_name} image[{idx}]: height got={} expected={expected_h}",
            got.height
        );
        // 이미지 크기 2000 HWPUNIT = 20pt 계약 (fixture 설계 상수 — 실측 확인).
        assert!(
            support::approx_eq(expected_w, 20.0, 0.01) && support::approx_eq(expected_h, 20.0, 0.01),
            "{base}/{path_name} image[{idx}]: fixture 설계 계약 2000 HWPUNIT=20pt 위반 (w={expected_w} h={expected_h})"
        );

        let expected_y = hwpunit_to_pt(exp.host_top_y);
        assert!(
            support::approx_eq(got.y, expected_y, 0.01),
            "{base}/{path_name} image[{idx}]: y got={} expected={expected_y} (margin_top+header_margin+host line vertpos)",
            got.y
        );

        let left = hwpunit_to_pt(exp.host_left_x);
        let right = hwpunit_to_pt(exp.host_right_x);
        if exp.para_is_image_only {
            // 선행 텍스트 없음 — x 는 줄 좌변과 정확히 일치해야 한다.
            assert!(
                support::approx_eq(got.x, left, 0.01),
                "{base}/{path_name} image[{idx}]: image-only 문단은 x==줄 좌변 정확해야 함 got={} expected={left}",
                got.x
            );
        } else {
            assert!(
                got.x >= left - 0.01 && got.x <= right + 0.01,
                "{base}/{path_name} image[{idx}]: x={} 는 [줄 좌변={left}, 줄 우변={right}] 구간 밖",
                got.x
            );
            eprintln!(
                "{base}/{path_name} image[{idx}]: x={} (line range [{left}, {right}], 선행 텍스트 shaping 의존 — 실측값 보고)",
                got.x
            );
        }
    }
}

// ── 테스트 ─────────────────────────────────────────────────────────

#[test]
fn inline_treat_as_char_png_body_renders_and_matches_layout_cache() {
    let Some(options) = font_options() else { return };
    if !fixture_path("inline_treat_as_char_png_body.hwpx").exists() {
        return;
    }
    let oracle = verify_oracle_and_load("inline_treat_as_char_png_body");
    let expected_image_count = oracle["images"].as_array().map_or(0, Vec::len);

    for (path_name, output, doc) in render_both_paths("inline_treat_as_char_png_body", &options) {
        assert_self_consistency("inline_treat_as_char_png_body", path_name, &output, &doc);
        let pages = support::extract_pages(&output.bytes);
        assert_eq!(
            pages[0].images.len(),
            expected_image_count,
            "{path_name}: oracle.json 이 선언한 이미지 개수와 불일치"
        );
    }
}

#[test]
fn inline_treat_as_char_jpeg_edges_renders_image_only_line_and_trailing_image() {
    let Some(options) = font_options() else { return };
    if !fixture_path("inline_treat_as_char_jpeg_edges.hwpx").exists() {
        return;
    }
    let oracle = verify_oracle_and_load("inline_treat_as_char_jpeg_edges");
    let expected_image_count = oracle["images"].as_array().map_or(0, Vec::len);

    for (path_name, output, doc) in render_both_paths("inline_treat_as_char_jpeg_edges", &options) {
        assert_self_consistency("inline_treat_as_char_jpeg_edges", path_name, &output, &doc);

        let pages = support::extract_pages(&output.bytes);
        assert_eq!(
            pages[0].images.len(),
            expected_image_count,
            "{path_name}: oracle.json 이 선언한 이미지 개수와 불일치"
        );
        // JPEG passthrough: XObject 필터가 /DCTDecode 여야 한다 (재인코드 없이 그대로 임베드).
        for (idx, img) in pages[0].images.iter().enumerate() {
            assert_eq!(
                img.filter.as_deref(),
                Some("DCTDecode"),
                "{path_name} image[{idx}]: JPEG 는 /DCTDecode passthrough 여야 함 — filter={:?}",
                img.filter
            );
        }
        // 최소 하나는 image-only 문단(para#0, 선행 텍스트 없음)이어야 한다.
        let expected = expected_body_images(&doc);
        assert!(
            expected.iter().any(|e| e.para_is_image_only),
            "{path_name}: jpeg_edges fixture 는 image-only 문단을 최소 1개 가져야 함"
        );
    }
}

/// 한컴 PDF runtime 대조 — 크기만 게이트(≤0.1pt), y/x 델타는 보고만.
#[test]
fn hancom_pdf_runtime_comparison_reports_position_deltas() {
    let Some(options) = font_options() else { return };
    for base in ["inline_treat_as_char_png_body", "inline_treat_as_char_jpeg_edges"] {
        if !fixture_path(&format!("{base}.hwpx")).exists() {
            continue;
        }
        verify_oracle_and_load(base);
        let hancom_pdf_path = fixture_path(&format!("{base}.pdf"));
        let hancom_bytes = std::fs::read(&hancom_pdf_path).expect("hancom pdf readable");
        let hancom_pages = support::extract_pages(&hancom_bytes);
        assert!(!hancom_pages.is_empty(), "{base}: 한컴 PDF 최소 1쪽");
        let hancom_images = &hancom_pages[0].images;

        let hwpx_bytes =
            std::fs::read(fixture_path(&format!("{base}.hwpx"))).expect("hwpx readable");
        let (ours, _doc) = render_hwpx_bytes(&hwpx_bytes, &options);
        let ours_pages = support::extract_pages(&ours.bytes);
        let ours_images = &ours_pages[0].images;

        assert_eq!(
            ours_images.len(),
            hancom_images.len(),
            "{base}: 우리 산출과 한컴 PDF 의 이미지 개수가 일치해야 함"
        );
        for (idx, (ours_img, hancom_img)) in
            ours_images.iter().zip(hancom_images.iter()).enumerate()
        {
            // 게이트: 크기(폭/높이) ≤0.1pt (실측 최소 허용치 — serializer 정밀도).
            assert!(
                support::approx_eq(ours_img.width, hancom_img.width, 0.1),
                "{base} image[{idx}]: width ours={} hancom={} (허용 0.1pt)",
                ours_img.width,
                hancom_img.width
            );
            assert!(
                support::approx_eq(ours_img.height, hancom_img.height, 0.1),
                "{base} image[{idx}]: height ours={} hancom={} (허용 0.1pt)",
                ours_img.height,
                hancom_img.height
            );
            // 보고만: y/x 는 한컴 여백/폰트 메트릭 차이가 얼마인지 먼저
            // 실측해야 하므로 게이트로 잠그지 않는다 (§4 D3 "실측 최소
            // 허용치" 원칙).
            eprintln!(
                "{base} image[{idx}]: delta x={:+.3}pt y={:+.3}pt (ours=({:.3},{:.3}) hancom=({:.3},{:.3}))",
                ours_img.x - hancom_img.x,
                ours_img.y - hancom_img.y,
                ours_img.x,
                ours_img.y,
                hancom_img.x,
                hancom_img.y
            );
        }
    }
}

/// 시각 게이트 산출물 생성 (사용자 판정용) — `--ignored` 로 수동 실행.
#[test]
#[ignore = "visual gate artifact generation (writes to examples/hwp5_review/_verify)"]
fn generate_w2b_visual_gate_artifacts() {
    let Some(options) = font_options() else {
        panic!("Hancom font bundle required");
    };
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/_verify/pdf-w2b");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    for base in ["inline_treat_as_char_png_body", "inline_treat_as_char_jpeg_edges"] {
        let hwpx_bytes = std::fs::read(fixture_path(&format!("{base}.hwpx"))).expect("hwpx");
        let (output, _doc) = render_hwpx_bytes(&hwpx_bytes, &options);
        let path = out_dir.join(format!("{base}-ours-w2b.pdf"));
        std::fs::write(&path, &output.bytes).expect("write");
        let hancom_src = fixture_path(&format!("{base}.pdf"));
        let hancom_dst = out_dir.join(format!("{base}-hancom-w2b.pdf"));
        std::fs::copy(&hancom_src, &hancom_dst).expect("copy hancom pdf for side-by-side");
        println!("wrote {path:?} + {hancom_dst:?}");
    }
}
