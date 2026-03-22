use super::shared::{
    colored_cell, csi, ctrl_p, empty, p, runs_p, text_cell, CS_GRAY, CS_HEADING, CS_NORMAL,
    CS_RED_BOLD, CS_SMALL, CS_TITLE, PS_BODY, PS_CENTER, PS_LEFT,
};
use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::chart::{ChartData, ChartGrouping, ChartType, LegendPosition};
use hwpforge_core::control::Control;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{BeginNum, MasterPage, PageBorderFillEntry, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{ApplyPageType, Color, HwpUnit};

pub(crate) fn section4_charts_equations_advanced() -> Section {
    let mut paras: Vec<Paragraph> = vec![
        p("차트, 수식, 고급 기능", CS_TITLE, PS_CENTER),
        empty(),
        // ── 4.1 수식(Equation) ──
        p("4.1 수식 (Equation — HancomEQN)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "HWPX의 수식은 HancomEQN 스크립트 형식을 사용합니다. MathML이 아닌 자체 문법입니다:",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
        // 수식 1: 분수
        p("분수:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("{a + b} over {c + d}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 2: 제곱근
        p("제곱근:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("root {2} of {x^2 + y^2}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 3: 적분
        p("적분:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("int _{0} ^{inf} e^{-x^2} dx = {sqrt {pi}} over {2}"),
            CS_NORMAL,
            PS_CENTER,
        ),
        // 수식 4: 행렬
        p("행렬:", CS_NORMAL, PS_LEFT),
        ctrl_p(
            Control::equation("left ( matrix {a # b ## c # d} right )"),
            CS_NORMAL,
            PS_CENTER,
        ),
        empty(),
        // ── 4.2 차트(Chart) ──
        p("4.2 차트 (Chart — OOXML)", CS_HEADING, PS_LEFT),
        empty(),
        p(
            "HWPX는 OOXML(Office Open XML) 차트 형식을 사용합니다. Chart XML은 ZIP 내 별도 파일로 저장되며, content.hpf 매니페스트에는 등록하지 않습니다.",
            CS_NORMAL,
            PS_BODY,
        ),
        empty(),
        // 차트 1: 세로막대 (Column, Clustered, 제목+범례)
        p("세로막대 차트 (Column, Clustered):", CS_NORMAL, PS_LEFT),
    ];
    let col_data = ChartData::category(
        &["1분기", "2분기", "3분기", "4분기"],
        &[("매출", &[120.0, 180.0, 150.0, 210.0]), ("비용", &[80.0, 100.0, 95.0, 130.0])],
    );
    let mut col_chart = Control::chart(ChartType::Column, col_data);
    if let Control::Chart { ref mut title, ref mut legend, .. } = col_chart {
        *title = Some("분기별 매출/비용".to_string());
        *legend = LegendPosition::Bottom;
    }
    paras.push(ctrl_p(col_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 2: 원형 (Pie)
    paras.push(p("원형 차트 (Pie):", CS_NORMAL, PS_LEFT));
    let pie_data = ChartData::category(
        &["한국", "미국", "일본", "기타"],
        &[("시장점유율", &[35.0, 28.0, 22.0, 15.0])],
    );
    let mut pie_chart = Control::chart(ChartType::Pie, pie_data);
    if let Control::Chart { ref mut title, ref mut explosion, .. } = pie_chart {
        *title = Some("시장 점유율".to_string());
        *explosion = Some(15);
    }
    paras.push(ctrl_p(pie_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 3: 꺾은선 (Line, markers)
    paras.push(p("꺾은선 차트 (Line):", CS_NORMAL, PS_LEFT));
    let line_data = ChartData::category(
        &["1월", "2월", "3월", "4월", "5월", "6월"],
        &[
            ("서울", &[2.0, 4.0, 10.0, 17.0, 22.0, 26.0]),
            ("부산", &[5.0, 7.0, 12.0, 18.0, 23.0, 27.0]),
        ],
    );
    let mut line_chart = Control::chart(ChartType::Line, line_data);
    if let Control::Chart { ref mut title, ref mut show_markers, ref mut grouping, .. } = line_chart
    {
        *title = Some("월별 평균 기온".to_string());
        *show_markers = Some(true);
        *grouping = ChartGrouping::Standard;
    }
    paras.push(ctrl_p(line_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // 차트 4: 분산형 (Scatter)
    paras.push(p("분산형 차트 (Scatter):", CS_NORMAL, PS_LEFT));
    let scatter_data = ChartData::xy(&[(
        "측정값",
        &[1.0, 2.5, 3.0, 4.5, 5.0, 6.5, 7.0, 8.5],
        &[2.3, 3.1, 4.8, 5.2, 7.1, 6.8, 8.9, 9.5],
    )]);
    let mut scatter_chart = Control::chart(ChartType::Scatter, scatter_data);
    if let Control::Chart { ref mut title, .. } = scatter_chart {
        *title = Some("X-Y 상관 분석".to_string());
    }
    paras.push(ctrl_p(scatter_chart, CS_NORMAL, PS_CENTER));
    paras.push(empty());

    // ── 4.3 고급 표 서식 ──
    paras.push(p("4.3 고급 표 서식", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "표는 col_span으로 셀 병합, background로 배경색 지정이 가능합니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // 복합 표: HWPX 요소 분류
    let mut merged_title = TableCell::new(
        vec![p("HWPX 요소 분류표", CS_RED_BOLD, PS_CENTER)],
        HwpUnit::from_mm(170.0).unwrap(),
    );
    merged_title.col_span = 3;
    merged_title.background = Some(Color::from_rgb(240, 240, 200));

    let th_row = TableRow::new(vec![merged_title]);

    let th2_row = TableRow::new(vec![
        colored_cell("분류", 40.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
        colored_cell("요소명", 65.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
        colored_cell("설명", 65.0, CS_RED_BOLD, PS_CENTER, 230, 235, 245),
    ]);

    let r1 = TableRow::new(vec![
        colored_cell("구조", 40.0, CS_NORMAL, PS_CENTER, 250, 255, 250),
        text_cell("Section, Paragraph, Run", 65.0, CS_SMALL, PS_LEFT),
        text_cell("문서의 기본 골격 (섹션→문단→런)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r2 = TableRow::new(vec![
        colored_cell("서식", 40.0, CS_NORMAL, PS_CENTER, 250, 250, 255),
        text_cell("CharShape, ParaShape, Style", 65.0, CS_SMALL, PS_LEFT),
        text_cell("글자/문단 모양 정의 (header.xml)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r3 = TableRow::new(vec![
        colored_cell("객체", 40.0, CS_NORMAL, PS_CENTER, 255, 250, 245),
        text_cell("Table, Image, TextBox, Chart", 65.0, CS_SMALL, PS_LEFT),
        text_cell("인라인 또는 부동 객체", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r4 = TableRow::new(vec![
        colored_cell("도형", 40.0, CS_NORMAL, PS_CENTER, 255, 245, 250),
        text_cell("Line, Ellipse, Polygon, Arc, Curve", 65.0, CS_SMALL, PS_LEFT),
        text_cell("벡터 드로잉 객체 (shape common block)", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r5 = TableRow::new(vec![
        colored_cell("주석", 40.0, CS_NORMAL, PS_CENTER, 245, 250, 255),
        text_cell("Footnote, Endnote, Memo, Bookmark", 65.0, CS_SMALL, PS_LEFT),
        text_cell("참조 및 주석 체계", 65.0, CS_SMALL, PS_LEFT),
    ]);
    let r6 = TableRow::new(vec![
        colored_cell("필드", 40.0, CS_NORMAL, PS_CENTER, 255, 255, 240),
        text_cell("Hyperlink, Field, CrossRef, IndexMark", 65.0, CS_SMALL, PS_LEFT),
        text_cell("fieldBegin/fieldEnd 패턴 인코딩", 65.0, CS_SMALL, PS_LEFT),
    ]);

    let mut adv_table = Table::new(vec![th_row, th2_row, r1, r2, r3, r4, r5, r6]);
    adv_table.width = Some(HwpUnit::from_mm(170.0).unwrap());
    adv_table.caption = Some(Caption::new(
        vec![p("표 2. HWPX 문서 요소 분류", CS_SMALL, PS_CENTER)],
        CaptionSide::Bottom,
    ));

    paras.push(runs_p(vec![Run::table(adv_table, csi(CS_NORMAL))], PS_CENTER));
    paras.push(empty());

    // ── 4.4 페이지 테두리 + 시작 번호 ──
    paras.push(p("4.4 페이지 테두리 (PageBorderFill) + BeginNum", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "이 섹션에는 페이지 테두리(borderFillIDRef=3, 검은 실선)가 설정되어 있으며, 페이지 번호는 1부터 새로 시작합니다.",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // ── 4.5 종합 요약 ──
    paras.push(p("4.5 종합 요약", CS_HEADING, PS_LEFT));
    paras.push(empty());

    paras.push(p(
        "이 문서는 HwpForge 라이브러리의 전체 API를 사용하여 생성되었습니다. 4개 섹션에 걸쳐 다음 기능들을 시연했습니다:",
        CS_NORMAL,
        PS_BODY,
    ));
    paras.push(empty());

    // 기능 목록
    paras.push(p(
        "구조: Document, Section, Paragraph, Run, Table, Image(Store)",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "섹션: Header, Footer, PageNumber, ColumnSettings, Visibility, LineNumberShape",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "섹션: PageBorderFill, MasterPage, BeginNum, Gutter, Landscape",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "도형: Line, Ellipse, Polygon, Arc, Curve, ConnectLine, TextBox",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p(
        "스타일: ShapeStyle (rotation, flip, fill, arrow), Caption (4방향)",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(p("채우기: Solid, Gradient (Linear), Pattern (HorizontalLine)", CS_NORMAL, PS_LEFT));
    paras.push(p("차트: Column, Pie, Line, Scatter (OOXML 형식)", CS_NORMAL, PS_LEFT));
    paras.push(p("수식: fraction, root, integral, matrix (HancomEQN)", CS_NORMAL, PS_LEFT));
    paras.push(p("텍스트: Dutmal (3방향), Compose (글자겹침)", CS_NORMAL, PS_LEFT));
    paras.push(p("참조: Bookmark (Point/Span), CrossRef, Hyperlink", CS_NORMAL, PS_LEFT));
    paras.push(p("필드: ClickHere, Date, PageNum", CS_NORMAL, PS_LEFT));
    paras.push(p("주석: Footnote, Endnote, Memo, IndexMark", CS_NORMAL, PS_LEFT));
    paras.push(p("정렬: Left, Center, Right, Justify, Distribute", CS_NORMAL, PS_LEFT));
    paras.push(p(
        "스타일스토어: Font, CharShape(8종), ParaShape(5종), BorderFill(4종), Numbering, Tab",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p("=== HWPX 문서 구조 완전 가이드 끝 ===", CS_TITLE, PS_CENTER));

    // ── 섹션 설정 ──
    let mut section = Section::with_paragraphs(paras, PageSettings::a4());

    // 페이지 테두리
    section.page_border_fills = Some(vec![
        PageBorderFillEntry {
            apply_type: "BOTH".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
        PageBorderFillEntry {
            apply_type: "EVEN".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
        PageBorderFillEntry {
            apply_type: "ODD".to_string(),
            border_fill_id: 3,
            ..PageBorderFillEntry::default()
        },
    ]);

    // 시작 번호 리셋
    section.begin_num =
        Some(BeginNum { page: 1, footnote: 1, endnote: 1, pic: 1, tbl: 1, equation: 1 });

    // 마스터페이지 (워터마크)
    section.master_pages = Some(vec![MasterPage::new(
        ApplyPageType::Both,
        vec![p("[ DRAFT / 초안 ]", CS_GRAY, PS_CENTER)],
    )]);

    // 페이지 번호
    section.page_number = Some(PageNumber::bottom_center());

    section
}
