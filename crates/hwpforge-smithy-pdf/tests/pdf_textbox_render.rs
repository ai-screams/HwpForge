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
use hwpforge_smithy_pdf::source::{replay_layout, LaidLine, LineAtom, LineTextBox, ReplayLayout};
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

/// overflow 글상자는 **내용에 맞게 확장해** 그려진다 (§10g — 한컴 curSz
/// byte 증거·재투어 실차이 수정 F1). clip 은 잘림이 아니라 확장 경계 밖
/// 잉크 억제용으로 상존한다 — 양 경로 모두 clip 연산자를 방출해야 한다.
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

// ── W5 w1a: 글상자 내부 인라인 이미지 ─────────────────────────────

fn atom_kind(a: &LineAtom) -> &'static str {
    match a {
        LineAtom::Text(_) => "text",
        LineAtom::Image(_) => "image",
        LineAtom::TextBox(_) => "textbox",
    }
}

/// (direct-hwpx, hwp-carry) 두 경로의 배치(replay) 결과 — 폰트 불요(렌더가
/// 아니라 배치만). 두 경로 모두 내부 캐시를 가시 축으로 정규화한다.
fn replay_both(base: &str) -> [(&'static str, ReplayLayout); 2] {
    let hwpx = std::fs::read(review_dir().join(format!("{base}.hwpx"))).expect("hwpx readable");
    let direct = replay_bytes(&hwpx);

    let hwp = std::fs::read(review_dir().join(format!("{base}.hwp"))).expect("hwp readable");
    let convert_options = hwpforge_convert::ConvertOptions::default().with_carry_layout_cache(true);
    let (carried, _warnings) =
        hwpforge_convert::hwp5_to_hwpx_bytes_with_options(&hwp, convert_options)
            .expect("hwp5 -> hwpx carry conversion");
    let carry = replay_bytes(&carried);

    [("direct-hwpx", direct), ("hwp-carry", carry)]
}

fn replay_bytes(bytes: &[u8]) -> ReplayLayout {
    let decoded = HwpxDecoder::decode(bytes).expect("hwpx decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    replay_layout(&PdfInput { document: &validated, styles: &lookup }, &PdfOptions::default())
        .expect("replay")
}

/// 글상자 host 줄(원자에 TextBox 를 담은 줄)과 그 TextBox 를 찾는다.
fn find_textbox_line(layout: &ReplayLayout) -> (&LaidLine, &LineTextBox) {
    for page in &layout.pages {
        for line in &page.lines {
            for atom in &line.atoms {
                if let LineAtom::TextBox(tb) = atom {
                    return (line, tb);
                }
            }
        }
    }
    panic!("textbox atom 부재");
}

/// 내부 인라인 이미지가 양 경로에서 가시 축으로 정확 분할되는지 폰트 없이
/// (배치만) 잠근다. 렌더 게이트가 축 오분할을 `LineOverflow` 로 흘려보내는
/// 것과 달리, 이 게이트는 **정확 텍스트 대조로** 오분할을 큰 소리로 잡는다
/// (fixture 디코드 실측: 내부 캐시 [0, 23] = raw wire 31 − pic 8유닛).
#[test]
fn textbox_inline_image_splits_on_visible_axis() {
    let base = "textbox_inline_image-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재");
        return;
    }
    for (name, layout) in replay_both(base) {
        let (_, tb) = find_textbox_line(&layout);
        assert_eq!(tb.inner_lines.len(), 2, "{name}: 내부 2줄");
        let l0: Vec<_> = tb.inner_lines[0].atoms.iter().map(atom_kind).collect();
        assert_eq!(l0, ["text", "image", "text"], "{name}: 줄0 = 텍스트+이미지+텍스트");
        let l1: Vec<_> = tb.inner_lines[1].atoms.iter().map(atom_kind).collect();
        assert_eq!(l1, ["text"], "{name}: 줄1 = 텍스트");
        let LineAtom::Image(img) = &tb.inner_lines[0].atoms[1] else {
            panic!("{name}: 이미지 원자 부재")
        };
        assert_eq!((img.width, img.height), (3402, 3402), "{name}: 인라인 이미지 3402×3402");
        let LineAtom::Text(a) = &tb.inner_lines[0].atoms[0] else { panic!() };
        assert_eq!(a.text, "그림 앞 ", "{name}: 줄0 앞 텍스트");
        let LineAtom::Text(b) = &tb.inner_lines[0].atoms[2] else { panic!() };
        assert_eq!(b.text, " 그림 뒤 — 이 문장은 글상자 ", "{name}: 줄0 뒤 텍스트");
        let LineAtom::Text(c) = &tb.inner_lines[1].atoms[0] else { panic!() };
        assert_eq!(c.text, "폭에서 줄이 감기도록 길게 씁니다.", "{name}: 줄1 텍스트");
    }
}

/// 렌더 경로 — 내부 인라인 이미지가 실제 PDF 에 도달하고(정확히 1개), 표시
/// 크기가 원본(3402 HWPUNIT = 34.02pt)이며, bbox 가 글상자 사각형 안에 있고,
/// 페이지 수가 한컴 PDF 와 일치한다 (양 경로). 한컴 폰트 번들 필요.
#[test]
fn textbox_inline_image_reaches_pdf_inside_box() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom 폰트 번들 없음");
        return;
    };
    let base = "textbox_inline_image-base";
    if !fixture_present(base) {
        eprintln!("skip {base}: fixture 부재");
        return;
    }
    let hancom_pages = hancom_page_count(base);
    assert_eq!(hancom_pages, 1, "{base}: 한컴 PDF 1쪽");
    let boxes = replay_both(base);
    let rendered = render_both(base, &options);
    for ((name, layout), (_, out)) in boxes.iter().zip(rendered.iter()) {
        assert!(out.bytes.starts_with(b"%PDF-"), "{name}: PDF 헤더");
        assert!(warnings_are_known(&out.warnings), "{name}: 예상 밖 경고 {:?}", out.warnings);
        let pages = support::extract_pages(&out.bytes);
        assert_eq!(pages.len(), hancom_pages, "{name}: 쪽수 vs 한컴");
        let images: Vec<_> = pages.iter().flat_map(|p| &p.images).collect();
        assert_eq!(images.len(), 1, "{name}: 인라인 이미지 1개 도달");
        let img = images[0];
        assert!(
            support::approx_eq(img.width, 34.02, 0.5) && support::approx_eq(img.height, 34.02, 0.5),
            "{name}: 이미지 표시 크기 {}×{} ≈ 34.02pt (3402 HWPUNIT)",
            img.width,
            img.height
        );
        // 박스 사각형(페이지-공간 pt) = replay host 줄에서 유도 (Left/Justify
        // 모두 origin_x = horzpos). 이미지 bbox 는 이 안에 들어가야 한다.
        let (host, tb) = find_textbox_line(layout);
        let (bx, by) = (f64::from(host.line_box.horzpos) / 100.0, f64::from(host.top_y) / 100.0);
        let (bw, bh) = (f64::from(tb.width) / 100.0, f64::from(tb.height) / 100.0);
        assert!(
            img.x >= bx - 0.5 && img.x + img.width <= bx + bw + 0.5,
            "{name}: 이미지 x-범위 [{}, {}] 가 박스 [{}, {}] 밖",
            img.x,
            img.x + img.width,
            bx,
            bx + bw
        );
        assert!(
            img.y >= by - 0.5 && img.y + img.height <= by + bh + 0.5,
            "{name}: 이미지 y-범위 [{}, {}] 가 박스 [{}, {}] 밖",
            img.y,
            img.y + img.height,
            by,
            by + bh
        );
    }
}
