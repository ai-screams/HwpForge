//! 종합 쇼케이스 fixture base — 이미지/글상자 에픽 전 기능 1문서 (한컴 재저장 공동 제작용).
//!
//! 재투어(2026-08-26) 통과 기능을 실제 있을 법한 "HwpForge 소개서" (~3쪽) 에
//! 전부 담는다: ① 본문 인라인 이미지(W2b) ② sub-line 작은 이미지 ③ styled
//! 글상자(W4) ④ 글상자 내부 인라인 이미지(W5 w1a) ⑤ 표 셀 이미지(W3)
//! ⑥ 앵커 이미지(W5 w1b, 양수 offset — 재저장에 살아남는 검증 조합).
//! overflow 글상자는 의도적으로 제외 (#49 수정 전 — 전용 fixture 소관).
//!
//! Usage: `cargo run -p hwpforge-smithy-hwpx --example gen_showcase_hwpforge`
//! (워크스페이스 루트에서 실행 — 산출물은 `examples/hwp5_review/showcase_hwpforge-base.hwpx`)

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::{Image, ImageFormat, ImageStore};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap};
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, Color, HwpUnit, ParaShapeIndex, VerticalAlign};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

const MASCOT: &str = "hwpforge-mascot.png";
const ICON: &str = "fixture-image.png";

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn text(t: &str, cs: CharShapeIndex) -> Paragraph {
    para(vec![Run::text(t, cs)])
}

fn img(name: &str, mm: f64) -> Image {
    Image::new(
        name,
        HwpUnit::from_mm(mm).expect("w"),
        HwpUnit::from_mm(mm).expect("h"),
        ImageFormat::Png,
    )
}

fn cell(t: &str, cs: CharShapeIndex, w_mm: f64) -> TableCell {
    TableCell::new(vec![text(t, cs)], HwpUnit::from_mm(w_mm).expect("w"))
}

fn header_cell(t: &str, cs: CharShapeIndex, w_mm: f64) -> TableCell {
    cell(t, cs, w_mm).with_background(Color::from_rgb(230, 230, 230))
}

#[allow(clippy::too_many_lines)]
fn main() {
    let path = "examples/hwp5_review/showcase_hwpforge-base.hwpx";
    // 재저장 공동 제작 흐름: 이미 존재하면 절대 덮어쓰지 않는다
    // (재저장본 소실 사고 재발 방지 — 재생성하려면 파일을 먼저 지울 것).
    if std::path::Path::new(path).exists() {
        println!("SKIP (이미 존재 — 덮어쓰기 금지): {path}");
        return;
    }

    let mut store = style_store_for_preset("latest").expect("latest preset");
    let cs0 = CharShapeIndex::new(0); // 함초롬바탕 10pt
    let mut title = store.char_shape(cs0).expect("char shape 0").clone();
    title.height = HwpUnit::from_pt(18.0).expect("18pt");
    let cs_title = store.push_char_shape(title);
    let mut h2 = store.char_shape(cs0).expect("char shape 0").clone();
    h2.height = HwpUnit::from_pt(13.0).expect("13pt");
    let cs_h2 = store.push_char_shape(h2);

    let mut images = ImageStore::new();
    images.insert(
        MASCOT,
        std::fs::read("tests/fixtures/images/main-charactor.png").expect("mascot asset"),
    );
    images
        .insert(ICON, std::fs::read("examples/hwp5_review/fixture-image.png").expect("icon asset"));

    // ⑥ 앵커 이미지: 첫 소개 문단(긴 문단) 기준 오른쪽에 어울림(Square)
    //    배치. 양수 offset 만 사용 — 한컴 재저장이 API 음수 offset 을 0 으로
    //    클램프하는 실측(§9j) 때문에 음수는 전용 fixture 소관.
    //    제목 문단에 달면 마지막 줄이 이미지에 걸려 문단끝+1 빈 세그먼트가
    //    방출됨 (§10h 디코드 갭, task #50) — 마지막 줄이 이미지 세로 범위
    //    아래로 벗어나는 긴 문단에 앵커한다.
    let mut anchored = img(MASCOT, 18.0);
    anchored.placement = Some(ObjectPlacement {
        text_wrap: ObjectTextWrap::Square,
        text_flow: ObjectTextFlow::BothSides,
        treat_as_char: false,
        flow_with_text: false,
        allow_overlap: true,
        vert_rel_to: ObjectRelativeTo::Para,
        horz_rel_to: ObjectRelativeTo::Para,
        vert_offset: HwpUnit::from_mm(2.0).expect("voff"),
        horz_offset: HwpUnit::from_mm(130.0).expect("hoff"),
    });

    let styled_box = |title_line: &str, body: &str, h_mm: f64| {
        para(vec![Run::control(
            Control::TextBox {
                paragraphs: vec![text(title_line, cs0), text(body, cs0)],
                width: HwpUnit::from_mm(120.0).expect("width"),
                height: HwpUnit::from_mm(h_mm).expect("height"),
                placement: None,
                caption: None,
                style: Some(ShapeStyle {
                    line_color: Some(Color::from_rgb(0, 0, 255)),
                    fill_color: Some(Color::from_rgb(255, 244, 200)),
                    line_width: Some(100),
                    ..Default::default()
                }),
                text_vertical_align: VerticalAlign::Top,
            },
            cs0,
        )])
    };

    let paragraphs = vec![
        // ── 1쪽: 소개 ──────────────────────────────────────────────
        text("HwpForge — 한글 문서를 코드로 담금질하다", cs_title),
        // 앵커 마커는 문단 **중간**에 둔다 (§10h 규칙: 분할 행 오른쪽에
        // 항상 후속 텍스트가 차야 문단끝+1 빈 세그먼트가 안 생김).
        para(vec![
            Run::text(
                "HwpForge 는 한국의 표준 문서 형식인 HWP/HWPX 를 프로그래밍으로 \
                 읽고, 쓰고, 편집하는 Rust 라이브러리입니다. ",
                cs0,
            ),
            Run::image(anchored, cs0),
            Run::text(
                "사람이 한글 프로그램에서 하던 일 — 서식 문서를 열고, 빈칸을 \
                 채우고, 표를 고치고, PDF 로 내보내는 일 — 을 코드와 AI \
                 에이전트가 대신할 수 있도록 처음부터 LLM-first 원칙으로 \
                 설계했습니다. 오른쪽에 떠 있는 마스코트가 바로 어울림(Square) \
                 배치로 앵커된 이미지이고, 이 문단의 글줄이 이미지를 피해 \
                 흐릅니다. 문단이 이미지 높이보다 길게 이어지므로 아래쪽 \
                 글줄은 다시 전체 폭으로 돌아옵니다.",
                cs0,
            ),
        ]),
        para(vec![
            Run::text(
                "정부 제안서, 지원 사업 신청서, 공문서처럼 한글 서식이 \
                 표준인 곳에서 문서 작업을 자동화하는 것이 목표입니다. \
                 마스코트 ",
                cs0,
            ),
            Run::image(img(MASCOT, 12.0), cs0),
            Run::text(
                " 처럼 문장 안에 글자처럼 끼어드는 이미지도, 문서 여백에 \
                 떠 있는 이미지도, 원본 조판 그대로 PDF 로 재현합니다. \
                 이 문서 자체가 HwpForge 의 API 로 생성된 뒤 한글 \
                 프로그램에서 재저장된 실물 시연입니다.",
                cs0,
            ),
        ]),
        para(vec![
            Run::text("체크 표시 ", cs0),
            Run::image(img(ICON, 3.0), cs0),
            Run::text(
                " 같은 작은 아이콘은 줄 높이를 해치지 않고 글줄 바닥 쪽에 \
                 내려앉습니다 — 한컴과 동일한 하강 비율(실측 상수)로 \
                 배치되므로, 아이콘이 섞인 문장도 줄 간격이 흔들리지 \
                 않습니다.",
                cs0,
            ),
        ]),
        text("왜 HwpForge 인가", cs_h2),
        text(
            "HWP 생태계는 수십 년의 역사만큼 형식이 복잡합니다. HWP5 는 \
             바이너리 레코드 구조이고, HWPX 는 KS X 6101 표준의 XML \
             패키지이지만 실제 한컴오피스의 구현은 표준 문서와 다른 지점이 \
             수십 곳입니다. HwpForge 는 이 간극을 실측으로 메꿉니다 — \
             네이티브 한컴 산출물을 바이트 단위로 대조해 검증한 것만 \
             구현하고, 확인되지 않은 값은 임의 기본값으로 덮지 않고 경고를 \
             내보냅니다.",
            cs0,
        ),
        text(
            "이 원칙 덕분에 문서를 열었다 저장해도 원본 조판이 유지되고, \
             지원하지 않는 요소는 조용히 사라지는 대신 명시적 경고로 \
             드러납니다. 자동화 파이프라인에서 가장 무서운 것은 오류가 \
             아니라 무음 손실이기 때문입니다.",
            cs0,
        ),
        styled_box(
            "핵심 원칙 — 구조와 스타일의 분리",
            "Core 는 문서 구조만, Blueprint 는 YAML 스타일 템플릿만 \
             가집니다. HTML 과 CSS 의 관계처럼, 하나의 스타일 템플릿을 \
             여러 문서에 입힐 수 있고 같은 문서를 여러 서식으로 출력할 수 \
             있습니다.",
            32.0,
        ),
        text(
            "예를 들어 기관 공통 서식(글꼴·자간·제목 체계)을 YAML 템플릿 \
             하나로 정의해 두면, Markdown 으로 쓴 초안 수십 건을 같은 \
             서식의 HWPX 로 일괄 생산할 수 있습니다. 반대로 이미 완성된 \
             HWPX 에서 구조만 추출해 다른 템플릿을 입히는 것도 같은 \
             원리로 동작합니다.",
            cs0,
        ),
        // ── 2쪽: 아키텍처 ──────────────────────────────────────────
        text("아키텍처 — 대장간 은유", cs_h2),
        para(vec![Run::control(
            Control::TextBox {
                paragraphs: vec![para(vec![
                    Run::text("기초 자재 ", cs0),
                    Run::image(img(ICON, 6.0), cs0),
                    Run::text(
                        " 위에서 Core(모루)가 문서 구조를 받치고, \
                         Blueprint(설계도)가 스타일을 정의하며, Smithy(화덕) \
                         들이 형식별로 최종 문서를 벼려 냅니다. 이 상자 안의 \
                         그림처럼, 글상자 내부의 이미지·줄바꿈도 원본 조판 \
                         좌표로 재생됩니다.",
                        cs0,
                    ),
                ])],
                width: HwpUnit::from_mm(120.0).expect("width"),
                height: HwpUnit::from_mm(35.0).expect("height"),
                placement: None,
                caption: None,
                style: None,
                text_vertical_align: VerticalAlign::Top,
            },
            cs0,
        )]),
        text(
            "워크스페이스는 대장간의 분업처럼 계층화되어 있습니다. 아래 \
             계층은 위 계층을 모르고, 위 계층은 아래의 공개 API 만 \
             사용합니다.",
            cs0,
        ),
        para(vec![Run::table(
            Table::new(vec![
                TableRow::new(vec![
                    header_cell("크레이트", cs0, 40.0),
                    header_cell("역할", cs0, 110.0),
                ]),
                TableRow::new(vec![
                    cell("foundation", cs0, 40.0),
                    cell(
                        "기초 자료형 — HwpUnit(정수 단위계)·Color(BGR)·branded \
                         index. 실수 오차와 인덱스 혼용을 타입으로 차단",
                        cs0,
                        110.0,
                    ),
                ]),
                TableRow::new(vec![
                    cell("core", cs0, 40.0),
                    TableCell::new(
                        vec![para(vec![
                            Run::text("순수 문서 구조 ", cs0),
                            Run::image(img(ICON, 8.0), cs0),
                            Run::text(
                                " — 문단·표·이미지·글상자. 셀 안 이미지도 행 \
                                 높이에 반영되어 이렇게 실립니다",
                                cs0,
                            ),
                        ])],
                        HwpUnit::from_mm(110.0).expect("w"),
                    ),
                ]),
                TableRow::new(vec![
                    cell("blueprint", cs0, 40.0),
                    cell("YAML 스타일 템플릿 — 상속·부분 정의·해석 파이프라인", cs0, 110.0),
                ]),
                TableRow::new(vec![
                    cell("smithy-*", cs0, 40.0),
                    cell(
                        "형식 컴파일러 — HWPX·HWP5·Markdown·PDF. Core 구조에 \
                         Blueprint 스타일을 융합해 최종 바이트를 산출",
                        cs0,
                        110.0,
                    ),
                ]),
                TableRow::new(vec![
                    cell("bindings-*", cs0, 40.0),
                    cell("CLI·MCP(AI 에이전트용)·Python 인터페이스", cs0, 110.0),
                ]),
            ]),
            cs0,
        )]),
        text(
            "설계 전반에 타입 안전 장치가 깔려 있습니다. 색상은 HWP 형식 \
             고유의 BGR 바이트 순서를 내부에 숨기고 RGB 생성자만 노출하며, \
             길이는 1pt = 100 단위의 정수 HwpUnit 으로 계산해 부동소수점 \
             누적 오차를 원천 차단합니다. 문자 모양·문단 모양·글꼴 인덱스는 \
             팬텀 타입으로 구분되어 서로 대입하면 컴파일이 거부되고, 검증을 \
             통과하지 않은 문서는 타입 상태(typestate) 때문에 저장 API \
             자체를 호출할 수 없습니다.",
            cs0,
        ),
        text(
            "HWP5 바이너리는 5단계 파이프라인(패키지 → 레코드 → 스키마 → \
             투영 → 방출)으로 해석해 HWPX 와 동일한 Core 구조로 수렴시키고, \
             변환 과정에서 원본의 조판 캐시(줄 배치 좌표)를 선별 보존해 \
             재저장 후에도 쪽수와 줄바꿈이 밀리지 않습니다.",
            cs0,
        ),
        // ── 3쪽: 기능·품질 ─────────────────────────────────────────
        text("무엇을 할 수 있나", cs_h2),
        text(
            "문서 생산: Markdown + YAML 템플릿에서 HWPX 를 생성합니다. \
             GFM 표·목록·작업 목록이 한글 문서의 표·번호 매기기·확인란 \
             글머리로 자연스럽게 대응됩니다. 문서 변환: HWP5 를 HWPX 로 \
             변환하며, 도형·수식·메모·각주·미주·상호참조·문서 정보까지 \
             바이트 검증된 범위를 그대로 운반합니다.",
            cs0,
        ),
        text(
            "AI 편집: 누름틀(클릭히어 필드) 채우기, 표 격자 주소로 셀 \
             수정, 문단 삽입·삭제, 템플릿 스탬핑을 델타 API 로 제공합니다. \
             모든 편집은 원본 바이트를 최대한 보존하는 스플라이스 방식이라 \
             편집하지 않은 부분의 조판이 흔들리지 않고, 역델타 자가 검증으로 \
             편집 결과를 스스로 확인합니다. MCP 서버를 통해 Claude 같은 AI \
             에이전트가 이 기능들을 자연어로 부릴 수 있습니다.",
            cs0,
        ),
        text(
            "PDF 내보내기: 한글 프로그램이 문서에 남긴 조판 캐시를 재생해 \
             PDF 를 생성합니다. 새로 조판하는 것이 아니라 원본 좌표를 \
             재생하므로, 쪽수·줄바꿈·표 분할·이미지 위치가 한컴 출력과 \
             일치합니다. 이 문서에 실린 인라인 이미지·앵커 이미지·글상자·표 \
             셀 이미지가 전부 그 재생 경로로 렌더된 실증 사례입니다.",
            cs0,
        ),
        styled_box(
            "숫자로 보는 품질",
            "한컴 원본 왕복(golden) 테스트 · 3,300+ 자동 테스트 · 커버리지 \
             90% 이상 CI 게이트 · 정부 문서 말뭉치 2,200여 건 전수 회귀 — \
             모든 렌더 결과는 한컴 PDF 와 0.1pt 이내 대조로 검증합니다.",
            30.0,
        ),
        text("릴리스 이력 (요약)", cs_h2),
        para(vec![Run::table(
            Table::new(vec![
                TableRow::new(vec![
                    header_cell("버전", cs0, 30.0),
                    header_cell("주요 내용", cs0, 120.0),
                ]),
                TableRow::new(vec![
                    cell("0.11.x", cs0, 30.0),
                    cell("AI 편집 에픽 — 누름틀·델타 fill·표 격자·문단 편집·스탬핑", cs0, 120.0),
                ]),
                TableRow::new(vec![
                    cell("0.12–0.13", cs0, 30.0),
                    cell(
                        "PDF 내보내기 기반 — 조판 캐시 재생·표 렌더·폰트 파이프라인·CLI",
                        cs0,
                        120.0,
                    ),
                ]),
                TableRow::new(vec![
                    cell("0.14", cs0, 30.0),
                    cell("좌표 원장(ledger) — 마커·가시 좌표 통일, 캐시 정합 진단", cs0, 120.0),
                ]),
                TableRow::new(vec![
                    cell("0.15", cs0, 30.0),
                    cell("이미지 렌더 — 본문 인라인·표 셀·배치 판정·bbox 정량 게이트", cs0, 120.0),
                ]),
                TableRow::new(vec![
                    cell("0.16", cs0, 30.0),
                    cell(
                        "글상자·앵커 — 배치 공용화·내부 재생·클리핑·앵커 이미지·sub-line",
                        cs0,
                        120.0,
                    ),
                ]),
            ]),
            cs0,
        )]),
        text(
            "앞으로는 이미지 형식 정책(BMP·WMF·EMF), Markdown 경로의 이미지 \
             임베드, 말뭉치 전수 성적표로 이번 에픽을 마감하고, 장평·자간 \
             재현과 그라데이션 채움 같은 충실도 항목을 이어갑니다.",
            cs0,
        ),
        text(
            "여기까지 — 이 문서에 실린 제목 옆 앵커 마스코트, 문장 속 인라인 \
             마스코트, 줄 바닥의 작은 아이콘, 색 입힌 글상자, 글상자 안 \
             그림, 표 셀 속 그림이 이미지/글상자 에픽이 배포한 기능 전부를 \
             한 문서에서 시연한 것입니다.",
            cs0,
        ),
    ];

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &images).expect("encode");
    std::fs::write(path, &bytes).expect("write");
    println!("생성: {path} ({} bytes)", bytes.len());
    println!();
    println!("한컴오피스에서 할 일 (재저장 = 조판 캐시 생성):");
    println!("  1. showcase_hwpforge-base.hwpx 열기 → 전 요소 확인 (~3쪽)");
    println!("  2. 같은 이름으로 재저장 (.hwpx) + PDF 내보내기 (-base.pdf)");
}
