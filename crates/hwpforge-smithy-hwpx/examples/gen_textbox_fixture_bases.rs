//! W4 글상자 fixture base 스캐폴드 — 한컴 재저장 공동 제작용 2종.
//!
//! W4 게이트가 요구하는 실측 계약: ① 박스 배치(offset 해석) ② 내부
//! lineseg 재생(셀 클리핑 선례 재사용) ③ 세로정렬(vert_align). 기존
//! rect_simple 은 실측 결과 진짜 글상자가 아니어서 대체 fixture 를 만든다.
//!
//! Usage: `cargo run -p hwpforge-smithy-hwpx --example gen_textbox_fixture_bases`
//! (워크스페이스 루트에서 실행 — 산출물은 `examples/hwp5_review/*-base.hwpx`)

use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, VerticalAlign};
use hwpforge_smithy_hwpx::presets::style_store_for_preset;
use hwpforge_smithy_hwpx::HwpxEncoder;

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn textbox_para(
    inner: Vec<Paragraph>,
    width_mm: f64,
    height_mm: f64,
    valign: VerticalAlign,
) -> Paragraph {
    Paragraph::with_runs(
        vec![Run::control(
            Control::TextBox {
                paragraphs: inner,
                width: HwpUnit::from_mm(width_mm).expect("width"),
                height: HwpUnit::from_mm(height_mm).expect("height"),
                placement: None,
                caption: None,
                style: None,
                text_vertical_align: valign,
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    )
}

fn save(name: &str, paragraphs: Vec<Paragraph>) {
    let path = format!("examples/hwp5_review/{name}-base.hwpx");
    // 재저장 공동 제작 흐름에서 사용자가 같은 이름으로 한컴 재저장본을
    // 남기므로, 이미 존재하면 절대 덮어쓰지 않는다 (재저장본 소실 사고
    // 재발 방지 — 재생성하려면 파일을 먼저 지울 것).
    if std::path::Path::new(&path).exists() {
        println!("SKIP (이미 존재 — 덮어쓰기 금지): {path}");
        return;
    }
    let store = style_store_for_preset("latest").expect("latest preset");
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    let validated = doc.validate().expect("validate");
    let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).expect("encode");
    std::fs::write(&path, &bytes).expect("write");
    println!("생성: {path} ({} bytes)", bytes.len());
}

fn main() {
    // ① 계약 실측용: 내부 텍스트가 여러 줄로 감기는 글상자 + 앞뒤 문단.
    //    한컴 재저장으로 박스 배치 좌표·내부 lineseg 좌표계(textMargin
    //    기본 283, gotcha #29)·호스트 줄 계정의 wire 진리를 얻는다.
    save(
        "textbox_basic",
        vec![
            text_para("글상자 앞 문단입니다."),
            textbox_para(
                vec![
                    text_para(
                        "글상자 안 첫 문단 — 이 문장은 글상자 폭에서 여러 줄로 \
                         감기도록 충분히 길게 씁니다. 내부 줄 좌표 계약 실측용.",
                    ),
                    text_para("안 둘째 문단(짧게)."),
                ],
                80.0,
                40.0,
                VerticalAlign::Top,
            ),
            text_para("글상자 뒤 문단입니다."),
        ],
    );

    // ② 세로정렬 실측용: 동일 크기 글상자 3개 (Top/Center/Bottom, 내부
    //    1줄) — ListHeader bits 5-6 / drawText subList vertAlign 대조.
    let mut valign_paras = vec![text_para("세로정렬 대조 문서입니다.")];
    for (label, valign) in [
        ("위 정렬 글상자:", VerticalAlign::Top),
        ("가운데 정렬 글상자:", VerticalAlign::Center),
        ("아래 정렬 글상자:", VerticalAlign::Bottom),
    ] {
        valign_paras.push(text_para(label));
        valign_paras.push(textbox_para(vec![text_para("한 줄 내용")], 60.0, 25.0, valign));
    }
    valign_paras.push(text_para("문서 끝 문단입니다."));
    save("textbox_valign", valign_paras);

    // ③ 오버플로 실측용: 내용(5줄 이상)이 박스(60×15mm)보다 큰 글상자 —
    //    한컴이 넘친 줄을 자르는지/lineseg 를 어디까지 방출하는지 실측
    //    (W3 셀 클리핑 선례 재사용 vs fail-closed 정책 결정 근거).
    save(
        "textbox_overflow",
        vec![
            text_para("오버플로 대조 문서입니다."),
            textbox_para(
                vec![
                    text_para(
                        "이 글상자는 일부러 내용보다 작게 만듭니다. 첫 문장부터 \
                         박스 폭에서 감기며, 다섯 줄을 넘겨 박스 높이를 초과하게 \
                         충분히 길게 이어 씁니다. 넘친 줄이 잘리는지, 조판 \
                         캐시에 넘친 줄의 lineseg 가 남는지가 실측 대상입니다.",
                    ),
                    text_para("넘침 이후 둘째 문단."),
                ],
                60.0,
                15.0,
                VerticalAlign::Top,
            ),
            text_para("글상자 뒤 문단입니다."),
        ],
    );

    // ④ 박스 페인트 실측용: 테두리색+채움색+선굵기 지정 글상자 —
    //    렌더러의 박스 자체 페인트(테두리/채움) 게이트 정답지.
    save(
        "textbox_styled",
        vec![
            text_para("스타일 대조 문서입니다."),
            Paragraph::with_runs(
                vec![Run::control(
                    Control::TextBox {
                        paragraphs: vec![text_para("파란 테두리, 연노랑 채움.")],
                        width: HwpUnit::from_mm(70.0).expect("width"),
                        height: HwpUnit::from_mm(20.0).expect("height"),
                        placement: None,
                        caption: None,
                        style: Some(ShapeStyle {
                            line_color: Some(hwpforge_foundation::Color::from_rgb(0, 0, 255)),
                            fill_color: Some(hwpforge_foundation::Color::from_rgb(255, 244, 200)),
                            line_width: Some(100), // ≈0.35mm
                            ..Default::default()
                        }),
                        text_vertical_align: VerticalAlign::Top,
                    },
                    CharShapeIndex::new(0),
                )],
                ParaShapeIndex::new(0),
            ),
            text_para("글상자 뒤 문단입니다."),
        ],
    );

    // ⑤ 앵커형 스캐폴드: 본문 두 문단만 — 글상자는 사용자가 한컴에서
    //    직접 삽입한다 (글자처럼 취급 해제·어울림·쪽 30mm/문단 10mm).
    //    API 인코더의 앵커 배치 의미가 미확정이라 wire 진리는 한컴 제작만.
    save(
        "textbox_anchored",
        vec![text_para("글상자 앞 문단입니다."), text_para("글상자 뒤 문단입니다.")],
    );

    println!();
    println!("한컴오피스에서 할 일 (재저장 = 조판 캐시·wire 진리 생성, 하나씩):");
    println!("  1. textbox_basic-base.hwpx 열기 → 글상자·내부 줄바꿈 확인");
    println!("  2. 같은 이름 3형식 저장: textbox_basic.hwp / .hwpx / PDF");
    println!("  3. textbox_valign-base.hwpx 도 동일 (세로정렬 3종 확인)");
}
