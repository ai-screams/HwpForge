//! W1 sub-line 인라인 이미지 렌더 e2e — 텍스트-지배 줄에 얹힌 작은 인라인
//! 이미지의 세로 배치(§10c-2 공식)를 한컴 PDF CTM 대조로 잠근다.
//!
//! fixture 는 리뷰 산출물 영역(`examples/hwp5_review/`)의 한컴 재저장본 —
//! `subline_image_v2-base`(10pt 줄×{2mm,3mm}·20pt 줄×{3mm,5mm}·글상자 내부
//! ×3mm = 5측점)와 `subline_image-base`(2측점). 한컴 폰트 번들 + fixture 가
//! 있는 머신에서만 실행된다(`pdf_textbox_render.rs` 와 동일한 fixture-optional
//! 관례 — CI(Linux·무한컴·미추적 fixture)에선 early-return).
//!
//! 게이트: 양 경로(직접 hwpx·hwp carry) 렌더 성공·기지 경고만·쪽수 1=1·양
//! 경로 이미지 bbox 동치·**y/w/h 를 한컴 PDF CTM 과 ≤0.1pt 대조**. y 는 W1
//! 이 도입한 sub-line 세로 배치 공식(§10c-2)의 산출값이라 여기서 잠근다. x 는
//! 앞선 텍스트 셰이핑 의존(자간·장평 미carry, W1 무관)이라 한컴 절대값과
//! 게이트하지 않고 델타를 보고하며, 두 경로 동치 + 본문 가로 범위로만 잠근다.

mod support;

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_pdf::{render_document, PdfInput, PdfOptions, PdfWarning};

const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

/// 한컴 PDF CTM 대조 허용치 (실측 최소치 — serializer 정밀도 + 크기 계통차).
const TOL_PT: f64 = 0.1;

fn review_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review")
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

fn fixture_present(base: &str) -> bool {
    ["hwpx", "hwp", "pdf"].iter().all(|ext| review_dir().join(format!("{base}.{ext}")).exists())
}

/// 허용 경고 — 폰트 폴백·정렬 근사·줄넘침(자간·장평 미carry)만 기지 경고.
fn warnings_are_known(warnings: &[PdfWarning]) -> bool {
    warnings.iter().all(|w| {
        matches!(
            w,
            PdfWarning::AlignmentApproximated { .. }
                | PdfWarning::LineOverflow { .. }
                | PdfWarning::FontStyleFallback { .. }
                | PdfWarning::FontAxisFallback { .. }
                | PdfWarning::MissingGlyphs { .. }
                | PdfWarning::FontEmbedPreviewPrint { .. }
        )
    })
}

fn render_hwpx(bytes: &[u8], options: &PdfOptions) -> hwpforge_smithy_pdf::PdfOutput {
    let decoded = HwpxDecoder::decode(bytes).expect("hwpx decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    render_document(&PdfInput { document: &validated, styles: &lookup }, options).expect("render")
}

/// (direct-hwpx, hwp-carry) 두 경로 산출물 — carry 는 `.hwp` → HWPX 변환
/// (`carry_layout_cache=true`) 후 렌더.
fn render_both(
    base: &str,
    options: &PdfOptions,
) -> [(&'static str, hwpforge_smithy_pdf::PdfOutput); 2] {
    let hwpx = std::fs::read(review_dir().join(format!("{base}.hwpx"))).expect("hwpx readable");
    let direct = render_hwpx(&hwpx, options);

    let hwp = std::fs::read(review_dir().join(format!("{base}.hwp"))).expect("hwp readable");
    let convert_options = hwpforge_convert::ConvertOptions::default().with_carry_layout_cache(true);
    let (carried, _warnings) =
        hwpforge_convert::hwp5_to_hwpx_bytes_with_options(&hwp, convert_options)
            .expect("hwp5 -> hwpx carry conversion");
    let carry = render_hwpx(&carried, options);

    [("direct-hwpx", direct), ("hwp-carry", carry)]
}

/// 본문 가로 범위 `[margin_left, width − margin_right]` (pt) — 이미지 x 의
/// 구조 sanity 경계 (한컴 절대 x 는 셰이핑 의존이라 대조하지 않는다).
fn body_extent_pt(base: &str) -> (f64, f64) {
    let hwpx = std::fs::read(review_dir().join(format!("{base}.hwpx"))).expect("hwpx");
    let decoded = HwpxDecoder::decode(&hwpx).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let ps = &validated.sections()[0].page_settings;
    let left = f64::from(ps.margin_left.as_i32()) / 100.0;
    let right = f64::from(ps.width.as_i32() - ps.margin_right.as_i32()) / 100.0;
    (left, right)
}

/// 한 fixture 의 5(또는 2)측점 이미지 bbox 를 한컴 PDF 와 대조한다.
///
/// y/w/h 는 ≤0.1pt 로 **게이트**한다 — y 는 이 슬라이스가 도입한 sub-line
/// 세로 배치 공식(§10c-2)의 산출값이라 여기서 잠가야 한다 (W2b 이미지-지배
/// e2e 는 y 가 자명히 `line_top` 이라 w/h 만 게이트했지만, sub-line 은 y 가
/// 공식 결과라 다르다). x 는 앞선 텍스트 셰이핑(자간·장평 미carry) 의존이고
/// W1 이 건드리지 않으므로 한컴 절대값과 게이트하지 않고 **보고**만 하되,
/// 두 렌더 경로 동치 + 본문 가로 범위로 구조적으로 잠근다.
fn assert_fixture_matches_hancom(base: &str, options: &PdfOptions) {
    let hancom_bytes = std::fs::read(review_dir().join(format!("{base}.pdf"))).expect("hancom pdf");
    let hancom_pages = support::extract_pages(&hancom_bytes);
    assert_eq!(hancom_pages.len(), 1, "{base}: 한컴 PDF 는 1쪽 fixture");
    let hancom_imgs = &hancom_pages[0].images;
    assert!(!hancom_imgs.is_empty(), "{base}: 한컴 PDF 에 이미지가 있어야 함");
    let (body_left, body_right) = body_extent_pt(base);

    let mut prev_path: Option<Vec<(f64, f64, f64, f64)>> = None;
    for (name, out) in render_both(base, options) {
        assert!(out.bytes.starts_with(b"%PDF-"), "{base}/{name}: PDF 헤더");
        assert!(
            warnings_are_known(&out.warnings),
            "{base}/{name}: 예상 밖 경고 {:?}",
            out.warnings
        );
        let pages = support::extract_pages(&out.bytes);
        assert_eq!(pages.len(), 1, "{base}/{name}: 쪽수 1=1 (한컴 대조)");
        let ours = &pages[0].images;
        assert_eq!(
            ours.len(),
            hancom_imgs.len(),
            "{base}/{name}: 이미지 개수 ours={} hancom={}",
            ours.len(),
            hancom_imgs.len()
        );

        // 양 경로 bbox 동치 (carry 가 직접 hwpx 와 같은 배치를 내야 함).
        let this_path: Vec<(f64, f64, f64, f64)> =
            ours.iter().map(|i| (i.x, i.y, i.width, i.height)).collect();
        if let Some(prev) = &prev_path {
            for (idx, (a, b)) in prev.iter().zip(&this_path).enumerate() {
                assert!(
                    (a.0 - b.0).abs() < 0.01
                        && (a.1 - b.1).abs() < 0.01
                        && (a.2 - b.2).abs() < 0.01
                        && (a.3 - b.3).abs() < 0.01,
                    "{base} image[{idx}]: 두 렌더 경로 bbox 불일치 {a:?} vs {b:?}"
                );
            }
        }
        prev_path = Some(this_path);

        // 한컴 PDF CTM 대조 — y/w/h 게이트(≤0.1pt), x 는 보고 + 구조 경계.
        //
        // ⚠️ 글상자-내부 이미지(v2 의 마지막 점 E)는 body 대비 y 가 +약 9u
        // (~0.09pt) 계통 이탈한다 — 박스 border 폭(33u) 미반영 코드 갭 후보
        // (적대 리뷰 r2 실증). 상수 k=0.152 에서 실측 Δy ≈ −0.072pt 로 ≤0.1pt
        // 게이트에 들어오나 여유가 크지 않다. 근본 해소·상수 vs 비율 판별은
        // w2 v3 fixture(다른 글꼴 패밀리 1점·border=0 글상자 1점) 재측정 몫.
        // 이 게이트가 깨지면 E 점만 ≤0.15pt 로 완화하고 사유를 여기 남긴다.
        for (idx, (o, h)) in ours.iter().zip(hancom_imgs.iter()).enumerate() {
            eprintln!(
                "{base}/{name} image[{idx}]: Δx={:+.4} Δy={:+.4} Δw={:+.4} Δh={:+.4}pt \
                 (ours=({:.4},{:.4},{:.4},{:.4}) hancom=({:.4},{:.4},{:.4},{:.4}))",
                o.x - h.x,
                o.y - h.y,
                o.width - h.width,
                o.height - h.height,
                o.x,
                o.y,
                o.width,
                o.height,
                h.x,
                h.y,
                h.width,
                h.height,
            );
            assert!(
                support::approx_eq(o.y, h.y, TOL_PT),
                "{base}/{name} image[{idx}]: y ours={} hancom={} (>{TOL_PT}pt — 세로 배치 공식)",
                o.y,
                h.y
            );
            assert!(
                support::approx_eq(o.width, h.width, TOL_PT),
                "{base}/{name} image[{idx}]: width ours={} hancom={} (>{TOL_PT}pt)",
                o.width,
                h.width
            );
            assert!(
                support::approx_eq(o.height, h.height, TOL_PT),
                "{base}/{name} image[{idx}]: height ours={} hancom={} (>{TOL_PT}pt)",
                o.height,
                h.height
            );
            // x 는 게이트하지 않고(셰이핑 의존) 본문 가로 범위 안인지만 확인.
            assert!(
                o.x >= body_left - 1.0 && o.x <= body_right + 1.0,
                "{base}/{name} image[{idx}]: x={} 는 본문 가로 범위 [{body_left}, {body_right}] 밖 (구조 오류)",
                o.x
            );
        }
    }
}

/// v2 fixture — 5측점(10pt 줄×{2mm,3mm}·20pt 줄×{3mm,5mm}·글상자 내부 3mm).
#[test]
fn subline_image_v2_five_points_match_hancom() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    let base = "subline_image_v2-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재 (리뷰 산출물 미추적)");
        return;
    }
    assert_fixture_matches_hancom(base, &options);
}

/// v1 fixture — 2측점(본문 3mm + 글상자 내부 3mm).
#[test]
fn subline_image_v1_two_points_match_hancom() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    let base = "subline_image-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재 (리뷰 산출물 미추적)");
        return;
    }
    assert_fixture_matches_hancom(base, &options);
}

/// 시각 게이트 산출물(사용자 판정용) — `--ignored` 로 수동 실행. 우리 렌더와
/// 한컴 재저장본을 나란히 `examples/hwp5_review/_verify/pdf-subline-w1/` 에 쓴다.
#[test]
#[ignore = "visual gate artifact generation (writes to examples/hwp5_review/_verify)"]
fn generate_subline_visual_gate_artifacts() {
    let Some(options) = font_options() else {
        panic!("Hancom 폰트 번들 필요");
    };
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/_verify/pdf-subline-w1");
    std::fs::create_dir_all(&out_dir).expect("mkdir");
    for base in ["subline_image_v2-base", "subline_image-base"] {
        if !fixture_present(base) {
            eprintln!("skip {base}: fixture 부재");
            continue;
        }
        let (_, ours) = &render_both(base, &options)[0];
        let ours_path = out_dir.join(format!("{base}-ours-w1.pdf"));
        std::fs::write(&ours_path, &ours.bytes).expect("write ours");
        let hancom_dst = out_dir.join(format!("{base}-hancom.pdf"));
        std::fs::copy(review_dir().join(format!("{base}.pdf")), &hancom_dst).expect("copy hancom");
        println!("wrote {ours_path:?} + {hancom_dst:?}");
    }
}
