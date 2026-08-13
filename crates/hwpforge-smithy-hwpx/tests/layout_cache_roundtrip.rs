//! W1c 게이트: 인코더 opt-in 캐시 방출.
//!
//! - 기본 인코딩(양 진입점)은 `<hp:linesegarray>` 를 방출하지 않는다
//!   (편집 표면 불변 — 바이트 수준 중립성은 기존 고정점 게이트가 잠근다).
//! - `emit_layout_cache` opt-in 은 Core 로 승격된 캐시를 wire 로 되돌려
//!   방출하고, 재디코드 시 본문·표 셀·머리말 캐시가 전부 보존돼야 한다.

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::ImageStore;
use hwpforge_core::layout::{LayoutCache, LineSeg};
use hwpforge_core::page::PageSettings;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{style_store_for_preset, EncodeOptions, HwpxDecoder, HwpxEncoder};

fn seg(textpos: u32, vertpos: i32) -> LineSeg {
    LineSeg {
        textpos,
        vertpos,
        vertsize: 1000,
        textheight: 1000,
        baseline: 850,
        spacing: 600,
        horzpos: 0,
        horzsize: 48188,
        flags: 0x0060_0000,
    }
}

fn cached_para(text: &str, lines: Vec<LineSeg>) -> Paragraph {
    let mut p =
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    p.layout_cache = Some(LayoutCache::new(lines));
    p
}

/// 본문(2줄 캐시) + 표 셀(1줄) + 머리말(1줄) — 컨테이너별 방출 경로 검증.
fn build_doc() -> Document<Draft> {
    let cell = TableCell::new(
        vec![cached_para("셀 본문", vec![seg(0, 0)])],
        HwpUnit::from_pt(100.0).unwrap(),
    );
    let mut host = cached_para("본문 문단", vec![seg(0, 0), seg(3, 1600)]);
    host.add_run(Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0)));

    let mut section = Section::with_paragraphs(vec![host], PageSettings::a4());
    section.headers.push(HeaderFooter::all_pages(vec![cached_para("머리말", vec![seg(0, 800)])]));

    let mut doc = Document::new();
    doc.add_section(section);
    doc
}

fn section_xml(bytes: &[u8]) -> String {
    use std::io::Read as _;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("zip");
    let mut xml = String::new();
    archive
        .by_name("Contents/section0.xml")
        .expect("section0")
        .read_to_string(&mut xml)
        .expect("utf-8");
    xml
}

#[test]
fn default_encode_omits_linesegarray() {
    // 주의: 같은 문서라도 인코딩마다 generate_instid() 산출 id 가 달라
    // 두 encode 호출의 바이트 동일성은 성립하지 않는다. 옵션 플럼빙의
    // 기본 경로 중립성은 기존 바이트 고정점 게이트(994 테스트)가 잠근다.
    // 여기서는 "기본 = 캐시 미방출" 불변식만 두 진입점 모두에서 잠근다.
    let store = style_store_for_preset("default").expect("preset");
    let validated = build_doc().validate().expect("validate");
    let images = ImageStore::new();

    let plain = HwpxEncoder::encode(&validated, &store, &images).expect("encode");
    assert!(!section_xml(&plain).contains("linesegarray"), "기본 인코딩은 캐시를 방출하지 않는다");

    let with_default =
        HwpxEncoder::encode_with_options(&validated, &store, &images, EncodeOptions::default())
            .expect("encode_with_options");
    assert!(
        !section_xml(&with_default).contains("linesegarray"),
        "EncodeOptions::default() 도 캐시를 방출하지 않는다"
    );
}

#[test]
fn optin_emits_linesegarray_and_roundtrips_all_containers() {
    let store = style_store_for_preset("default").expect("preset");
    let validated = build_doc().validate().expect("validate");
    let bytes = HwpxEncoder::encode_with_options(
        &validated,
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect("encode opt-in");

    let xml = section_xml(&bytes);
    assert!(xml.contains("<hp:linesegarray>"), "opt-in 은 캐시를 방출한다: {xml}");

    // 재디코드 → 승격된 캐시가 컨테이너별로 전부 살아있는지
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let mut caches: Vec<(String, LayoutCache)> = Vec::new();
    let mut doc = decoded.document;
    doc.for_each_paragraph_mut(|p| {
        if let Some(c) = &p.layout_cache {
            caches.push((p.text_content(), c.clone()));
        }
    });

    let get = |needle: &str| -> &LayoutCache {
        &caches
            .iter()
            .find(|(t, _)| t.contains(needle))
            .unwrap_or_else(|| panic!("{needle} 캐시 없음: {caches:?}"))
            .1
    };
    assert_eq!(get("본문 문단").lines, vec![seg(0, 0), seg(3, 1600)], "본문 2줄 캐시 보존");
    assert_eq!(get("셀 본문").lines, vec![seg(0, 0)], "표 셀 캐시 보존");
    assert_eq!(get("머리말").lines, vec![seg(0, 800)], "머리말 캐시 보존");
}

// ── W1b 매트릭스: marker/tab/field 왕복 + 실패 의미 잠금 ────────────

fn roundtrip(doc: Document<Draft>) -> hwpforge_smithy_hwpx::EncodeOutcome {
    let store = style_store_for_preset("default").expect("preset");
    let validated = doc.validate().expect("validate");
    HwpxEncoder::encode_with_diagnostics(
        &validated,
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect("encode")
}

fn decoded_caches(bytes: &[u8]) -> Vec<Option<Vec<u32>>> {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    decoded.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| p.layout_cache.as_ref().map(|c| c.lines.iter().map(|l| l.textpos).collect()))
        .collect()
}

#[test]
fn marker_paragraph_roundtrips_visible_coordinates() {
    // 중간 각주 marker(8,0) 를 낀 2줄 문단 — Core 가시 tp [0, 4] 가
    // encode(가시→wire) → decode(wire→가시) 왕복에서 항등이어야 한다.
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![
            Run::text("가나다", CharShapeIndex::new(0)),
            Run::control(
                Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text("각주", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]),
                CharShapeIndex::new(0),
            ),
            Run::text("라마바", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(4, 1600)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    let outcome = roundtrip(doc);
    assert!(outcome.warnings.is_empty(), "무경고: {:?}", outcome.warnings);
    // wire 방출 실측: 첫 lineseg 는 native 불변식대로 0, 둘째는
    // secPr(8)+colPr(8) 주입 + 텍스트 3 + marker 8 + 1 = 20+8+... —
    // 재디코드 가시 좌표 항등만 잠근다 (wire 값은 ledger 소유).
    assert_eq!(decoded_caches(&outcome.bytes)[0], Some(vec![0, 4]));
}

#[test]
fn tab_paragraph_roundtrips_visible_coordinates() {
    // tab (8,1) 비대칭 — 가시 [0, 3] ("ab\t" 뒤 줄) 왕복 항등.
    let mut para = Paragraph::with_runs(
        vec![Run::text("ab\tcd", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(3, 1600)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    let outcome = roundtrip(doc);
    assert!(outcome.warnings.is_empty(), "무경고: {:?}", outcome.warnings);
    assert_eq!(decoded_caches(&outcome.bytes)[0], Some(vec![0, 3]));
}

#[test]
fn field_paragraph_roundtrips_when_lines_avoid_body() {
    // ClickHere 필드(본문 접힘 (16+n,0)) — 줄 경계가 본문 밖이면 왕복 항등.
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![
            Run::text("라벨: ", CharShapeIndex::new(0)),
            Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text: Some("이름".into()),
                    help_text: None,
                    name: Some("이름".into()),
                    display_text: "홍길동".into(),
                },
                CharShapeIndex::new(0),
            ),
            Run::text(" 끝", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    // 줄2 = " 끝" 내부 (core 5) — core 4 는 필드 축약점(begin 앞/end 뒤
    // 동좌표)이라 모호 → 드롭이 정답 (아래 별도 테스트).
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(5, 1600)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    let outcome = roundtrip(doc);
    assert!(outcome.warnings.is_empty(), "무경고: {:?}", outcome.warnings);
    assert_eq!(decoded_caches(&outcome.bytes)[0], Some(vec![0, 5]));
}

#[test]
fn unencodable_control_drops_cache_with_typed_warning() {
    // §1g v5 R3#6 일반 불변식: 방출이 스킵되는 컨트롤(Unknown 등)이 있는
    // 문단은 기하가 stale — typed 경고 + 해당 문단만 linesegarray 부재.
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![
            Run::text("본문 앞", CharShapeIndex::new(0)),
            Run::control(
                Control::Unknown { tag: "zzzz".into(), data: None },
                CharShapeIndex::new(0),
            ),
        ],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

    let outcome = roundtrip(doc);
    assert!(
        outcome.warnings.iter().any(|w| matches!(
            w,
            hwpforge_smithy_hwpx::EncodeWarning::LayoutCacheDropped { reason, .. }
                if reason.contains("unencodable")
        )),
        "드롭 경고: {:?}",
        outcome.warnings
    );
    assert_eq!(decoded_caches(&outcome.bytes)[0], None, "스킵 문단 캐시 미방출");
}

#[test]
fn legacy_strict_entrypoint_errors_on_cache_drop() {
    // §1g v5 변경 2: emit_layout_cache 요청 중 드롭 = legacy 진입점은
    // 무음 성공 대신 HwpxError::LayoutCacheDropped.
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![Run::control(
            Control::Unknown { tag: "zzzz".into(), data: None },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let store = style_store_for_preset("default").expect("preset");
    let validated = doc.validate().expect("validate");

    let err = HwpxEncoder::encode_with_options(
        &validated,
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect_err("strict 진입점은 에러");
    assert!(
        matches!(&err, hwpforge_smithy_hwpx::HwpxError::LayoutCacheDropped { .. }),
        "got {err:?}"
    );
}

// ── 독립 리뷰 Medium 상환: 대칭-버그를 잡는 절대값 oracle ──────────

/// 방출 XML 의 linesegarray textpos 절대값을 §1f 실측 상수로 대조한다.
///
/// roundtrip(encode∘decode) 은 **대칭 오류**(예: 양쪽 다 marker 를
/// 7유닛으로 세는 버그)를 항등으로 통과시킨다 — 이 oracle 의 기대값은
/// 코드가 아니라 한컴 fixture 실측(§1f: 확장 marker = 8 wire 유닛,
/// 첫 lineseg = 0)에서 유도된 하드코딩 상수라 그 대칭을 깬다.
#[test]
fn emitted_wire_textpos_matches_hancom_measured_constants() {
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![
            Run::text("가나다", CharShapeIndex::new(0)),
            Run::control(
                Control::footnote(vec![Paragraph::with_runs(
                    vec![Run::text("각주", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )]),
                CharShapeIndex::new(0),
            ),
            Run::text("라마바", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(4, 1600)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let store = style_store_for_preset("default").expect("preset");
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode_with_options(
        &validated,
        &store,
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .expect("encode");
    let xml = section_xml(&bytes);
    // 한컴 실측 유도 상수: 줄1 = 0 (native 전수 불변식) · 줄2 =
    // secPr(8)+colPr(8) 주입 + "가나다"(3) + 각주 marker(8) + "라"(1) = 28.
    assert!(xml.contains(r#"textpos="0""#), "line1 wire 0: {xml}");
    assert!(xml.contains(r#"textpos="28""#), "line2 wire 28 (16+3+8+1): {xml}");
}

/// 접힘 필드 본문의 tab = wire 8유닛 (독립 리뷰 Low 상환 fixture).
#[test]
fn folded_field_body_tab_counts_eight_wire_units() {
    use hwpforge_core::control::Control;
    let mut para = Paragraph::with_runs(
        vec![
            Run::text("ab", CharShapeIndex::new(0)),
            Run::control(
                Control::Field {
                    field_type: hwpforge_foundation::FieldType::ClickHere,
                    hint_text: Some("h".into()),
                    help_text: None,
                    name: Some("f".into()),
                    display_text: "x\ty".into(),
                },
                CharShapeIndex::new(0),
            ),
            Run::text("cd", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    para.layout_cache = Some(LayoutCache::new(vec![seg(0, 0), seg(3, 1600)]));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let outcome = roundtrip(doc);
    assert!(outcome.warnings.is_empty(), "무경고: {:?}", outcome.warnings);
    // 왕복 항등 + 절대값: 줄2 core 3("d") → wire = 16(주입) + "ab"(2)
    // + 필드(16 + x(1)+tab(8)+y(1) = 26) + "c"(1) = 45.
    assert_eq!(decoded_caches(&outcome.bytes)[0], Some(vec![0, 3]));
    let xml = section_xml(&outcome.bytes);
    assert!(xml.contains(r#"textpos="45""#), "tab=8 절대값 (16+2+26+1): {xml}");
}

/// 실물 한컴 fixture 골든 — 리뷰 영역 fixture 존재 시에만 실행 (930KB
/// 라 커밋 금지 규칙 대상; 부재 환경(CI)에선 skip 로그 후 통과).
#[test]
fn real_hancom_marker_ledger_fixture_decodes_to_measured_triples() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/hwp5_review/rules-marker-ledger-base.hwpx");
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("skip: {} 부재 (리뷰 영역 미보유 환경)", path.display());
        return;
    };
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let caches: Vec<Vec<u32>> = decoded.document.sections()[0]
        .paragraphs
        .iter()
        .filter_map(|p| p.layout_cache.as_ref())
        .filter(|c| c.lines.len() >= 2)
        .map(|c| c.lines.iter().map(|l| l.textpos).collect())
        .collect();
    // §1g 수용 기준 골든 (raw [0,76,132]/[0,64,117]/[0,62,117] 에서
    // 선행 marker 8×n 차감 유도 — Codex 4차 산술 재검증 완료).
    assert_eq!(
        caches,
        vec![vec![0, 60, 116], vec![0, 56, 109], vec![0, 54, 109]],
        "실물 한컴 fixture 가시 좌표 골든"
    );
}
