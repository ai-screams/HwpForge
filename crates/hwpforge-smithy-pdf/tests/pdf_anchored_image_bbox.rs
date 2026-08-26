//! W5 w1b — body 앵커형 이미지 bbox 정량 게이트 (§9e·§9c).
//!
//! `floating_image_not_treat_as_char` fixture(vertRelTo=PARA·vertOffset=4840·
//! TOP_AND_BOTTOM·treatAsChar=0)를 렌더해 `support::extract_pages` 로 앵커
//! 이미지의 페이지-공간 bbox 를 뽑고, **committed oracle sidecar**
//! (`anchored_image_oracle.json` — 네이티브 한컴 PDF 에서 1회 기록)의 실측
//! 값과 대조한다. 자체 일관성(placement 산술)도 함께 잠근다.
//!
//! oracle 를 sidecar 로 굳혀 1.1MB 한컴 PDF 를 커밋하지 않고도 실측 대조가
//! **CI-durable** 하다. `hwpx_sha256` 가 fixture drift 를, `hancom_pdf_sha256`
//! 가 기록 수치의 출처를 고정한다. 한컴 PDF 가 있으면 sidecar 수치가 신선
//! 추출과 일치하는지 belt-and-suspenders 로 재검한다.
//!
//! **검증 축 (advisor 확정)**: 이 fixture 는 **positive vert_offset ·
//! first-para(first_v=0) · horz_offset=0** 축만 oracle-검증한다. 음수 offset·
//! nonzero horz·non-first-para 는 w2 사용자 fixture 대기 (§9e — team-lead
//! 결정 사항).
//!
//! fixture(`tests/fixtures/images/`) 는 항상 존재하므로 SHA drift 검사는
//! 폰트 없이도 돈다. bbox 대조는 한컴 폰트 번들이 있는 머신에서만 실행된다
//! (pdf_image_placement_bbox.rs 와 동일 관례).

mod support;

use std::path::PathBuf;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_pdf::{render_document, PdfInput, PdfOptions};

const HANCOM_TTF_DIR: &str =
    "/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF";
const FIXTURE: &str = "floating_image_not_treat_as_char";

fn fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/images").join(name)
}

/// 한컴 PDF 정답지 (untracked 리뷰 자산 — 있으면 sidecar 재검용).
fn hancom_pdf_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hwp5_review").join(name)
}

fn font_options() -> Option<PdfOptions> {
    let dir = PathBuf::from(HANCOM_TTF_DIR);
    if !dir.exists() {
        return None;
    }
    let mut options = PdfOptions::default();
    options.font_dirs = vec![dir];
    options.discovery = hwpforge_smithy_pdf::font::FontDiscovery::HancomBundle;
    Some(options)
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

fn hwpunit_to_pt(v: i32) -> f64 {
    f64::from(v) / 100.0
}

/// bbox (top-left pt).
struct Bbox {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

/// 문서의 첫 body 앵커 이미지의 기대 bbox 를 margin + placement offset +
/// **마커 소속 lineseg vertpos** 로 독립 계산한다 (§10i F3 — floor-search;
/// 마커가 첫 줄이면 종전 first-lineseg 모델과 동치. 렌더 파이프라인과 무관
/// — self-consistency; 판별 oracle 은 한컴 실측이 담당).
fn expected_anchored(doc: &Document<Validated>) -> Bbox {
    for section in doc.sections() {
        let body_top = section.page_settings.margin_top.as_i32()
            + section.page_settings.header_margin.as_i32();
        let body_left = section.page_settings.margin_left.as_i32();
        for para in &section.paragraphs {
            let Some(cache) = para.layout_cache.as_ref() else { continue };
            if cache.lines.is_empty() {
                continue;
            }
            let mut marker_pos: u32 = 0;
            for run in &para.runs {
                if let RunContent::Image(img) = &run.content {
                    if let Some(p) = img.placement.as_ref().filter(|p| !p.treat_as_char) {
                        let idx =
                            cache.lines.iter().rposition(|s| s.textpos <= marker_pos).unwrap_or(0);
                        return Bbox {
                            x: hwpunit_to_pt(body_left + p.horz_offset.as_i32()),
                            y: hwpunit_to_pt(
                                body_top + cache.lines[idx].vertpos + p.vert_offset.as_i32(),
                            ),
                            width: hwpunit_to_pt(img.width.as_i32()),
                            height: hwpunit_to_pt(img.height.as_i32()),
                        };
                    }
                }
                if let Some(t) = run.content.plain_text() {
                    marker_pos += u32::try_from(t.encode_utf16().count()).expect("len");
                }
            }
        }
    }
    panic!("no body anchored image found in document");
}

fn load_oracle() -> serde_json::Value {
    let path = fixture_path("anchored_image_oracle.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("oracle sidecar unreadable: {e}"));
    serde_json::from_slice(&bytes).expect("oracle.json valid JSON")
}

fn oracle_bbox(entry: &serde_json::Value) -> Bbox {
    let b = &entry["hancom_image_bbox_pt"];
    Bbox {
        x: b["x"].as_f64().expect("x"),
        y: b["y"].as_f64().expect("y"),
        width: b["width"].as_f64().expect("width"),
        height: b["height"].as_f64().expect("height"),
    }
}

fn render_ours(options: &PdfOptions) -> Vec<u8> {
    let bytes = std::fs::read(fixture_path(&format!("{FIXTURE}.hwpx"))).expect("read hwpx");
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let input = PdfInput { document: &validated, styles: &lookup };
    render_document(&input, options).expect("render").bytes
}

/// fixture drift 검사 — sidecar 의 `hwpx_sha256` 가 실물 fixture 와 일치하는지.
/// 폰트 불요 (항상 실행 — CI 잠금).
#[test]
fn floating_fixture_matches_oracle_sha() {
    let oracle = load_oracle();
    let entry = &oracle["fixtures"][FIXTURE];
    let expected = entry["hwpx_sha256"].as_str().expect("hwpx_sha256");
    let bytes = std::fs::read(fixture_path(&format!("{FIXTURE}.hwpx"))).expect("read hwpx");
    assert_eq!(sha256_hex(&bytes), expected, "floating fixture hwpx drifted from oracle sidecar");
}

/// positive vert_offset · first-para · horz_offset=0 축을 sidecar 한컴 실측
/// 으로 잠근다 (advisor: 이 fixture 가 oracle-검증하는 유일 축).
#[test]
fn floating_anchored_image_bbox_matches_hancom_oracle() {
    let oracle = load_oracle();
    let entry = &oracle["fixtures"][FIXTURE];
    let hancom = oracle_bbox(entry);

    let Some(options) = font_options() else {
        eprintln!("skip: Hancom font bundle unavailable (SHA drift gate ran separately)");
        return;
    };

    // 렌더 → 앵커 이미지 bbox 추출.
    let ours_pdf = render_ours(&options);
    let pages = support::extract_pages(&ours_pdf);
    let our_imgs: Vec<_> = pages.iter().flat_map(|p| p.images.iter()).collect();
    assert_eq!(our_imgs.len(), 1, "정확히 한 앵커 이미지 렌더: {our_imgs:?}");
    let got = our_imgs[0];

    // 자체 일관성: 렌더 bbox == placement 산술 (±0.01pt — 순환이지만 pt변환/
    // 추출/오이미지/쪽 버그를 잡는다).
    let bytes = std::fs::read(fixture_path(&format!("{FIXTURE}.hwpx"))).expect("read");
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");
    let expected = expected_anchored(&validated);
    assert!(support::approx_eq(got.x, expected.x, 0.01), "x {} != {}", got.x, expected.x);
    assert!(support::approx_eq(got.y, expected.y, 0.01), "y {} != {}", got.y, expected.y);
    assert!(support::approx_eq(got.width, expected.width, 0.01), "w {}", got.width);
    assert!(support::approx_eq(got.height, expected.height, 0.01), "h {}", got.height);

    // 판별 게이트: 렌더 bbox == sidecar 한컴 실측 (±0.1pt — serializer 상한).
    eprintln!("ours   x={:.3} y={:.3} w={:.3} h={:.3}", got.x, got.y, got.width, got.height);
    eprintln!(
        "hancom x={:.3} y={:.3} w={:.3} h={:.3} (oracle sidecar)",
        hancom.x, hancom.y, hancom.width, hancom.height
    );
    eprintln!(
        "Δ x={:.3} y={:.3} w={:.3} h={:.3}",
        got.x - hancom.x,
        got.y - hancom.y,
        got.width - hancom.width,
        got.height - hancom.height
    );
    assert!(support::approx_eq(got.x, hancom.x, 0.1), "x ours {} vs hancom {}", got.x, hancom.x);
    assert!(support::approx_eq(got.y, hancom.y, 0.1), "y ours {} vs hancom {}", got.y, hancom.y);
    assert!(
        support::approx_eq(got.width, hancom.width, 0.1),
        "w ours {} vs hancom {}",
        got.width,
        hancom.width
    );
    assert!(
        support::approx_eq(got.height, hancom.height, 0.1),
        "h ours {} vs hancom {}",
        got.height,
        hancom.height
    );
}

/// belt-and-suspenders: 한컴 PDF 정답지가 있으면 sidecar 기록 수치가 신선
/// 추출과 일치하는지 재검한다 (기록값 drift 방어). PDF 부재 시 skip.
#[test]
fn oracle_sidecar_matches_fresh_hancom_extraction_when_present() {
    let oracle = load_oracle();
    let entry = &oracle["fixtures"][FIXTURE];
    let recorded = oracle_bbox(entry);
    let path = hancom_pdf_path(&format!("{FIXTURE}.pdf"));
    let Ok(pdf_bytes) = std::fs::read(&path) else {
        eprintln!(
            "skip: Hancom PDF oracle absent ({path:?}) — sidecar numbers unverified this run"
        );
        return;
    };
    let expected_sha = entry["hancom_pdf_sha256"].as_str().expect("hancom_pdf_sha256");
    assert_eq!(sha256_hex(&pdf_bytes), expected_sha, "Hancom PDF drifted from oracle sidecar");
    let pages = support::extract_pages(&pdf_bytes);
    let imgs: Vec<_> = pages.iter().flat_map(|p| p.images.iter()).collect();
    assert_eq!(imgs.len(), 1, "한컴 PDF 앵커 이미지 하나: {imgs:?}");
    let fresh = imgs[0];
    assert!(support::approx_eq(fresh.x, recorded.x, 0.001), "recorded x drifted");
    assert!(support::approx_eq(fresh.y, recorded.y, 0.001), "recorded y drifted");
    assert!(support::approx_eq(fresh.width, recorded.width, 0.001), "recorded w drifted");
    assert!(support::approx_eq(fresh.height, recorded.height, 0.001), "recorded h drifted");
}

/// §10i F3 — **마커 줄 판별 게이트** (커밋 fixture 쌍, hwpx+한컴 PDF 동커밋):
/// PARA 앵커의 수직 기준 = 마커 소속 줄. `anchored_marker_line` 은 마커가
/// 둘째 줄 **중간**(가시 60, 줄 tp [0,55,…) — 경고 없음), `anchored_marker_boundary`
/// 는 마커가 둘째 줄 **시작과 정확 일치**(가시 55 == tp 55 — 한컴도 아랫줄
/// 기준 배치를 byte-ground 확인, `ANCHOR_MARKER_ON_LINE_BOUNDARY` 표면화).
/// 두 fixture 모두 마지막 분할 행의 빈 오른쪽 세그먼트가 문단부호 sentinel
/// (wire끝+1)로 방출된 실물이라 **F2 디코드 수용의 회귀 게이트를 겸한다**.
#[test]
fn marker_line_anchored_images_match_hancom() {
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom font bundle unavailable");
        return;
    };
    for (name, expect_boundary_warning) in
        [("anchored_marker_line", false), ("anchored_marker_boundary", true)]
    {
        let bytes = std::fs::read(fixture_path(&format!("{name}.hwpx"))).expect("read hwpx");
        // F2 게이트: 문단부호 sentinel lineseg 가 있어도 디코드 드롭 0.
        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        let validated = decoded.document.validate().expect("validate");
        let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
        let out = render_document(&PdfInput { document: &validated, styles: &lookup }, &options)
            .expect("render");
        assert!(
            !out.warnings
                .iter()
                .any(|w| matches!(w, hwpforge_smithy_pdf::PdfWarning::ParagraphSkipped { .. })),
            "{name}: sentinel 문단이 스킵되면 F2 회귀: {:?}",
            out.warnings
        );
        let has_boundary = out.warnings.iter().any(|w| {
            matches!(w, hwpforge_smithy_pdf::PdfWarning::AnchorMarkerOnLineBoundary { .. })
        });
        assert_eq!(
            has_boundary, expect_boundary_warning,
            "{name}: 경계 경고 기대 {expect_boundary_warning}: {:?}",
            out.warnings
        );

        let pages = support::extract_pages(&out.bytes);
        let imgs: Vec<_> = pages.iter().flat_map(|p| p.images.iter()).collect();
        assert_eq!(imgs.len(), 1, "{name}: 앵커 이미지 하나 렌더: {imgs:?}");
        let got = imgs[0];

        // 자체 일관성: 마커 소속 줄 산술과 일치.
        let expected = expected_anchored(&validated);
        assert!(
            support::approx_eq(got.x, expected.x, 0.01),
            "{name} x {} != {}",
            got.x,
            expected.x
        );
        assert!(
            support::approx_eq(got.y, expected.y, 0.01),
            "{name} y {} != {}",
            got.y,
            expected.y
        );

        // 판별 게이트: 동커밋 한컴 PDF 신선 추출과 ±0.1pt.
        let hancom_pdf = std::fs::read(fixture_path(&format!("{name}.pdf"))).expect("hancom pdf");
        let hpages = support::extract_pages(&hancom_pdf);
        let himgs: Vec<_> = hpages.iter().flat_map(|p| p.images.iter()).collect();
        assert_eq!(himgs.len(), 1, "{name}: 한컴 PDF 앵커 이미지 하나: {himgs:?}");
        let h = himgs[0];
        eprintln!(
            "{name} Δ x={:.3} y={:.3} w={:.3} h={:.3}",
            got.x - h.x,
            got.y - h.y,
            got.width - h.width,
            got.height - h.height
        );
        assert!(support::approx_eq(got.x, h.x, 0.1), "{name} x ours {} vs hancom {}", got.x, h.x);
        assert!(support::approx_eq(got.y, h.y, 0.1), "{name} y ours {} vs hancom {}", got.y, h.y);
        assert!(support::approx_eq(got.width, h.width, 0.1), "{name} w {}", got.width);
        assert!(support::approx_eq(got.height, h.height, 0.1), "{name} h {}", got.height);
    }
}

/// W5 w2 — **음수 offset 한컴 oracle** (설계 리뷰 High-2): 대화상자 저작
/// fixture 로 음수 offset 산술을 한컴 PDF 와 직접 대조한다.
///
/// 한컴 저작 정규화 실측 (§9j): API 가 쓴 음수 vertOffset 은 재저장에서
/// 0 으로 클램프, 드래그는 **앵커 재지정**(첫 문단 + vert +534)으로
/// 표현, 음수는 **개체 속성 대화상자만** 직저작 가능 — HWPX 는 음수를
/// u32 랩 십진수로 인코드(`horzOffset="4294965029"` = −2267)하며
/// 디코더는 signed 복원. fixture 는 미추적 리뷰 자산 — 존재+폰트
/// 머신에서만 실행 (로컬 parity 게이트, CI 무신호).
#[test]
fn negative_horz_offset_anchored_image_matches_hancom() {
    let hwpx_path = hancom_pdf_path("anchored_negative_offset-base.hwpx");
    let pdf_path = hancom_pdf_path("anchored_negative_offset-base.pdf");
    if !hwpx_path.exists() || !pdf_path.exists() {
        eprintln!("skip: anchored_negative_offset 리뷰 fixture 부재");
        return;
    }
    let Some(options) = font_options() else {
        eprintln!("skip: Hancom font bundle unavailable");
        return;
    };

    let bytes = std::fs::read(&hwpx_path).expect("read hwpx");
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let validated = decoded.document.validate().expect("validate");

    // wire 잠금 — 대화상자 저작 음수 horz(−2267)·드래그 재지정 vert(+534),
    // u32 랩 십진수의 signed 복원까지 이 단정이 커버한다.
    let mut wire_locked = false;
    for section in validated.sections() {
        for para in &section.paragraphs {
            for run in &para.runs {
                if let RunContent::Image(img) = &run.content {
                    if let Some(p) = img.placement.as_ref().filter(|p| !p.treat_as_char) {
                        assert_eq!(p.horz_offset.as_i32(), -2267, "대화상자 저작 음수 horz");
                        assert_eq!(p.vert_offset.as_i32(), 534, "드래그 재지정 vert");
                        wire_locked = true;
                    }
                }
            }
        }
    }
    assert!(wire_locked, "anchored image placement not found");

    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let ours = render_document(&PdfInput { document: &validated, styles: &lookup }, &options)
        .expect("render")
        .bytes;
    let pages = support::extract_pages(&ours);
    let imgs: Vec<_> = pages.iter().flat_map(|p| p.images.iter()).collect();
    assert_eq!(imgs.len(), 1, "앵커 이미지 하나 렌더: {imgs:?}");
    let got = imgs[0];

    // 자체 일관성: 음수 horz 가 산술 x = body_left + horz_offset 로 흐른다.
    let expected = expected_anchored(&validated);
    assert!(support::approx_eq(got.x, expected.x, 0.01), "x {} != {}", got.x, expected.x);
    assert!(support::approx_eq(got.y, expected.y, 0.01), "y {} != {}", got.y, expected.y);

    // 판별 게이트: 한컴 PDF 신선 추출과 ±0.1pt.
    let hancom_pdf = std::fs::read(&pdf_path).expect("read hancom pdf");
    let hpages = support::extract_pages(&hancom_pdf);
    let himgs: Vec<_> = hpages.iter().flat_map(|p| p.images.iter()).collect();
    assert_eq!(himgs.len(), 1, "한컴 PDF 앵커 이미지 하나: {himgs:?}");
    let h = himgs[0];
    eprintln!(
        "neg-offset Δ x={:.3} y={:.3} w={:.3} h={:.3}",
        got.x - h.x,
        got.y - h.y,
        got.width - h.width,
        got.height - h.height
    );
    assert!(support::approx_eq(got.x, h.x, 0.1), "x ours {} vs hancom {}", got.x, h.x);
    assert!(support::approx_eq(got.y, h.y, 0.1), "y ours {} vs hancom {}", got.y, h.y);
    assert!(
        support::approx_eq(got.width, h.width, 0.1),
        "w ours {} vs hancom {}",
        got.width,
        h.width
    );
    assert!(
        support::approx_eq(got.height, h.height, 0.1),
        "h ours {} vs hancom {}",
        got.height,
        h.height
    );
}
