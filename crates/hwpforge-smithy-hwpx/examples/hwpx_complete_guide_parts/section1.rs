use super::shared::{
    colored_cell, csi, empty, p, runs_p, styled_p, text_cell, CS_BLUE, CS_GREEN_ITALIC, CS_HEADING,
    CS_NORMAL, CS_RED_BOLD, CS_SMALL, CS_TITLE, PS_BODY, PS_CENTER, PS_LEFT,
};
use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::Control;
use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{HwpUnit, NumberFormatType, PageNumberPosition, ParaShapeIndex};

pub(crate) fn section1_document_structure() -> Section {
    // 머리글/바닥글/페이지번호가 있는 A4 세로

    let mut paras: Vec<Paragraph> = vec![
        // ── 제목 ──
        styled_p(
            "HWPX 문서 구조 완전 가이드",
            CS_TITLE,
            PS_CENTER,
            0, // 바탕 스타일
        ),
        empty(),
    ];

    // ── 마스코트 이미지 + 캡션 ──
    let mut mascot_img = Image::from_path(
        "BinData/image1.png",
        HwpUnit::from_mm(60.0).unwrap(),
        HwpUnit::from_mm(60.0).unwrap(),
    );
    mascot_img.caption = Some(Caption::new(
        vec![p("[그림 1] 쇠부리 (SoeBuri) — 한컴 문서를 불에 달구어 단단하게 벼려내는 대장장이 오리너구리 🔥", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));
    paras.push(Paragraph::with_runs(
        vec![Run::image(mascot_img, csi(CS_NORMAL))],
        ParaShapeIndex::new(PS_CENTER),
    ));
    paras.push(empty());

    // ── 오리너구리 소개글 ──
    paras.push(runs_p(
        vec![
            Run::text("HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 ", csi(CS_NORMAL)),
            Run::text("순수 Rust 라이브러리", csi(CS_RED_BOLD)),
            Run::text("입니다. 프로젝트 마스코트인 ", csi(CS_NORMAL)),
            Run::text("오리너구리(Platypus)", csi(CS_RED_BOLD)),
            Run::text(
                "는 HWPX 포맷의 독특한 특성을 상징합니다 — XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.",
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 북마크: HWPX정의 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("HWPX정의"), csi(CS_NORMAL)),
            Run::control(Control::index_mark("HWPX"), csi(CS_NORMAL)),
            Run::text("1. HWPX 문서 포맷이란?", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    // ── HWPX 설명 + 하이퍼링크 + 각주 ──
    paras.push(runs_p(
        vec![
            Run::text("HWPX는 대한민국 국가표준 ", csi(CS_NORMAL)),
            Run::control(Control::index_mark("KS X 6101"), csi(CS_NORMAL)),
            Run::text("KS X 6101", csi(CS_BLUE)),
            Run::control(
                Control::footnote(vec![p(
                    "KS X 6101: 한국산업표준(Korean Industrial Standards)에서 제정한 문서 파일 형식 표준. 2014년 최초 제정, 2021년 개정.",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(
                "에 정의된 XML 기반 문서 포맷입니다. ZIP 컨테이너 안에 여러 XML 파일이 구조화되어 저장됩니다.",
                csi(CS_NORMAL),
            ),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 하이퍼링크 ──
    paras.push(runs_p(
        vec![
            Run::text("상세 사양은 ", csi(CS_NORMAL)),
            Run::control(
                Control::hyperlink("한국정보통신기술협회(TTA)", "https://www.tta.or.kr"),
                csi(CS_BLUE),
            ),
            Run::text(" 홈페이지에서 확인할 수 있습니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 북마크: 헤더구조 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("헤더구조"), csi(CS_NORMAL)),
            Run::text("2. ZIP 컨테이너 파일 구성", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p(
        "HWPX 파일은 확장자가 .hwpx인 ZIP 아카이브입니다. 내부에는 다음과 같은 XML 파일들이 포함됩니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 표: ZIP 파일 구성 ──
    let table_width = HwpUnit::from_mm(170.0).unwrap();
    let col_w1 = 55.0; // 파일명
    let col_w2 = 60.0; // 설명
    let col_w3 = 55.0; // 미디어타입

    // 헤더 행 (파란 배경)
    let header_row = TableRow::new(vec![
        colored_cell("파일 경로", col_w1, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
        colored_cell("설명", col_w2, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
        colored_cell("Media-Type", col_w3, CS_RED_BOLD, PS_CENTER, 220, 230, 245),
    ]);

    // 데이터 행
    let row1 = TableRow::new(vec![
        text_cell("META-INF/manifest.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("패키지 매니페스트 (파일 목록)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("text/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row2 = TableRow::new(vec![
        text_cell("Contents/content.hpf", col_w1, CS_SMALL, PS_LEFT),
        text_cell("콘텐츠 목차 (OPF)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row3 = TableRow::new(vec![
        text_cell("Contents/header.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("스타일 정의 (폰트, 문단, 글자)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row4 = TableRow::new(vec![
        text_cell("Contents/section0.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("본문 첫 번째 구획 (paragraphs)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    let row5 = TableRow::new(vec![
        text_cell("Contents/section1.xml", col_w1, CS_SMALL, PS_LEFT),
        text_cell("본문 두 번째 구획 (선택적)", col_w2, CS_SMALL, PS_LEFT),
        text_cell("application/xml", col_w3, CS_SMALL, PS_LEFT),
    ]);
    // col_span 행: BinData 설명
    let mut bindata_cell = text_cell(
        "BinData/ — 이미지, OLE 등 바이너리 데이터 폴더 (Content.hpf에 등록, Chart XML은 미등록)",
        col_w1 + col_w2 + col_w3,
        CS_GREEN_ITALIC,
        PS_LEFT,
    );
    bindata_cell.col_span = 3;
    let row6 = TableRow::new(vec![bindata_cell]);

    let mut tbl = Table::new(vec![header_row, row1, row2, row3, row4, row5, row6]);
    tbl.width = Some(table_width);
    tbl.caption = Some(Caption::new(
        vec![p("표 1. HWPX ZIP 컨테이너 내부 파일 구성", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));

    paras.push(runs_p(vec![Run::table(tbl, csi(CS_NORMAL))], PS_CENTER));
    paras.push(empty());

    // ── 북마크: 섹션구조 ──
    paras.push(runs_p(
        vec![
            Run::control(Control::bookmark("섹션구조"), csi(CS_NORMAL)),
            Run::control(
                Control::IndexMark {
                    primary: "OWPML".to_string(),
                    secondary: Some("섹션 구조".to_string()),
                },
                csi(CS_NORMAL),
            ),
            Run::text("3. 섹션(Section) 구조", csi(CS_HEADING)),
        ],
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p(
        "HWPX 문서는 하나 이상의 섹션으로 구성됩니다. 각 섹션은 독립적인 페이지 설정(용지 크기, 여백, 방향)을 가질 수 있어, 세로 페이지와 가로 페이지를 하나의 문서에 혼합할 수 있습니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    paras.push(p(
        "각 섹션의 XML은 <hp:sec> 루트 아래 <hp:p>(문단) 요소들로 구성됩니다. 문단 안에는 <hp:run>(텍스트 런), <hp:ctrl>(컨트롤), <hp:tbl>(표) 등이 포함됩니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 제목 4 ──
    paras.push(p("4. header.xml 스타일 시스템", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(runs_p(
        vec![
            Run::text("header.xml에는 문서 전체의 스타일 정의가 담깁니다: ", csi(CS_NORMAL)),
            Run::text("fontface(폰트)", csi(CS_RED_BOLD)),
            Run::text(", ", csi(CS_NORMAL)),
            Run::text("charShape(글자 모양)", csi(CS_RED_BOLD)),
            Run::text(", ", csi(CS_NORMAL)),
            Run::text("paraShape(문단 모양)", csi(CS_RED_BOLD)),
            Run::text(". 본문의 각 요소는 인덱스(IDRef)로 이 정의를 참조합니다.", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    // ── 각주 추가 설명 ──
    paras.push(runs_p(
        vec![
            Run::text(
                "스타일 정의 인덱스는 0부터 시작하며, Modern 스타일셋 기준으로 기본 charShape 7개, paraShape 20개가 자동 생성됩니다",
                csi(CS_NORMAL),
            ),
            Run::control(
                Control::footnote(vec![p(
                    "한글 2022(Modern 스타일셋)의 기본 스타일: charShape 0-6 (바탕~개요10), paraShape 0-19 (바탕~개요10). 사용자 정의 스타일은 이후 인덱스부터 시작합니다.",
                    CS_SMALL,
                    PS_BODY,
                )]),
                csi(CS_NORMAL),
            ),
            Run::text(".", csi(CS_NORMAL)),
        ],
        PS_BODY,
    ));
    paras.push(empty());

    paras.push(p(
        "이 문서는 HwpForge 라이브러리로 생성되었으며, 4개 섹션에 걸쳐 문서 포맷의 각 요소를 실제로 사용하면서 설명합니다.",
        CS_GREEN_ITALIC,
        PS_BODY,
    ));

    // ── 섹션 구성 ──
    let mut section = Section::with_paragraphs(paras, PageSettings::a4());

    // 머리글: 모든 페이지
    section.header = Some(HeaderFooter::all_pages(vec![p(
        "HWPX 문서 구조 완전 가이드 — HwpForge",
        CS_SMALL,
        PS_CENTER,
    )]));

    // 바닥글: 모든 페이지
    section.footer = Some(HeaderFooter::all_pages(vec![p(
        "Copyright 2026 HwpForge Project. All rights reserved.",
        CS_SMALL,
        PS_CENTER,
    )]));

    // 페이지 번호: 하단 가운데, "- N -" 형식
    section.page_number = Some(PageNumber::with_decoration(
        PageNumberPosition::BottomCenter,
        NumberFormatType::Digit,
        "- ",
    ));

    section
}
