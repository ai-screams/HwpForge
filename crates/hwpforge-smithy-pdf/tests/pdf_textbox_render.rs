//! W4 w3 — 인라인 글상자 렌더 e2e (fixture 4종 × 양 경로 + 한컴 PDF 쪽수
//! 대조 + overflow clip 존재 + anchored 음성 게이트).
//!
//! fixture 는 리뷰 산출물 영역(`examples/hwp5_review/`)의 한컴 재저장본 —
//! 미추적이라 **경로가 있을 때만** 실행한다. 렌더 경로 게이트는 한컴 폰트
//! 번들이 있는 머신에서만(`render_pdf.rs` 와 동일한 fixture-optional 관례).
//! anchored 음성 게이트는 admission 에서 거부되므로 폰트 없이도 돈다.
//!
//! ⚠️ **CI(Linux·무한컴·미추적 fixture)에선 4건 전부 early-return =
//! 무신호(passed 로 계수)** — 이 파일은 로컬 전용 parity 게이트다 (리뷰
//! Low-1). CI 커버는 소스 단위 테스트(clip 산술·admission·operator-level
//! clip)와 `anchored_textbox_stays_rejected` 음성 unit 이 담당한다.

mod support;

use std::path::PathBuf;

use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_pdf::{render_document, PdfError, PdfInput, PdfOptions, PdfWarning};

const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";

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

/// 허용 경고 — 폰트 폴백·정렬 근사·줄넘침(자간·장평 미carry)만 기지 경고.
/// 그 외(캐시 드롭·구조 경고 등)는 실패로 본다.
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

fn hancom_page_count(base: &str) -> usize {
    let pdf = std::fs::read(review_dir().join(format!("{base}.pdf"))).expect("hancom pdf readable");
    support::extract_pages(&pdf).len()
}

fn fixture_present(base: &str) -> bool {
    review_dir().join(format!("{base}.hwpx")).exists()
        && review_dir().join(format!("{base}.hwp")).exists()
        && review_dir().join(format!("{base}.pdf")).exists()
}

/// 4종 글상자 fixture 를 양 경로로 렌더해 성공·기지 경고·한컴 PDF 쪽수
/// 일치를 잠근다.
#[test]
fn textbox_fixtures_render_and_match_hancom_page_count() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    for base in [
        "textbox_basic-base",
        "textbox_valign-base",
        "textbox_overflow-base",
        "textbox_styled-base",
    ] {
        if !fixture_present(base) {
            eprintln!("skip {base}: fixture 부재 (리뷰 산출물 미추적)");
            continue;
        }
        let hancom_pages = hancom_page_count(base);
        assert!(hancom_pages >= 1, "{base}: 한컴 PDF 최소 1쪽");
        for (name, out) in render_both(base, &options) {
            assert!(out.bytes.starts_with(b"%PDF-"), "{base}/{name}: PDF 헤더");
            assert!(
                warnings_are_known(&out.warnings),
                "{base}/{name}: 예상 밖 경고 {:?}",
                out.warnings
            );
            let pages = support::extract_pages(&out.bytes);
            assert_eq!(pages.len(), hancom_pages, "{base}/{name}: 쪽수 vs 한컴");
        }
    }
}

/// overflow 글상자는 넘친 내용을 박스 경계로 절단한다 — 양 경로 모두
/// clip 연산자를 방출해야 한다 (clip 없으면 넘침이 안 잘린다).
#[test]
fn textbox_overflow_emits_clip_op() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    let base = "textbox_overflow-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재");
        return;
    }
    for (name, out) in render_both(base, &options) {
        assert!(
            support::count_clip_ops(&out.bytes) > 0,
            "{base}/{name}: overflow 글상자는 clip op 를 방출해야 한다"
        );
    }
}

/// styled fixture 의 박스 페인트 — 채움(#FFF4C8)은 `rg`, 테두리(#0000FF)
/// 는 `RG` 로 콘텐츠 스트림에 실제 방출돼야 한다 (W4 w4 페인트 게이트).
/// 존재만 보는 게 아니라 색 성분 3개를 ±0.002 로 대조한다.
///
/// **direct-hwpx 경로 한정**: HWP5 디코더는 GSO 도형의
/// lineShape/fillBrush 를 아직 캐리하지 않는다 (선재 갭 — 이 에픽 회귀
/// 아님, 명시 백로그). carry 경로는 내용+clip 만 렌더되므로 색 게이트를
/// 걸 수 없다.
#[test]
fn textbox_styled_paints_fill_and_border() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    let base = "textbox_styled-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재");
        return;
    }
    let hwpx = std::fs::read(review_dir().join(format!("{base}.hwpx"))).expect("hwpx readable");
    let out = render_hwpx(&hwpx, &options);
    assert!(
        support::count_color_ops(&out.bytes, (255, 244, 200), false) > 0,
        "{base}/direct-hwpx: 채움색 #FFF4C8 rg 방출"
    );
    assert!(
        support::count_color_ops(&out.bytes, (0, 0, 255), true) > 0,
        "{base}/direct-hwpx: 테두리색 #0000FF RG 방출"
    );
}

/// 음성 게이트 — 앵커형(treat_as_char=false) 글상자는 W4 범위 밖이라
/// 렌더가 fail-closed(InvalidCache)로 거부한다 (앵커 렌더 = W5). admission
/// 에서 거부되므로 폰트 없이도 검사한다.
#[test]
fn anchored_textbox_is_fail_closed() {
    let base = "textbox_anchored-base";
    let hwpx_path = review_dir().join(format!("{base}.hwpx"));
    if !hwpx_path.exists() {
        eprintln!("skip {base}: fixture 부재");
        return;
    }
    let options = font_options().unwrap_or_default();
    let hwpx = std::fs::read(&hwpx_path).expect("hwpx readable");
    let decoded = HwpxDecoder::decode(&hwpx).expect("hwpx decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let err = render_document(&PdfInput { document: &validated, styles: &lookup }, &options)
        .expect_err("anchored textbox must be rejected");
    assert!(matches!(err, PdfError::InvalidCache { .. }), "{err:?}");
}
