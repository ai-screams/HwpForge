use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::chart::{
    BarShape, ChartData, ChartGrouping, ChartType, LegendPosition, OfPieType, RadarStyle,
    ScatterStyle, StockVariant,
};
use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, PageNumber, Section};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{ApplyPageType, HwpUnit, NumberFormatType, PageNumberPosition};

use crate::{
    csi, empty, encode_and_save, mascot_intro, p, psi, showcase_store, CS_BLUE, CS_BOLD, CS_NORMAL,
    CS_RED, CS_SMALL, CS_TITLE, PS_CENTER, PS_LEFT, PS_RIGHT,
};

#[derive(Clone, Copy)]
struct ChartSizes {
    large_width: HwpUnit,
    large_height: HwpUnit,
    medium_width: HwpUnit,
    medium_height: HwpUnit,
}

impl ChartSizes {
    fn new() -> Self {
        Self {
            large_width: HwpUnit::from_mm(140.0).unwrap(),
            large_height: HwpUnit::from_mm(90.0).unwrap(),
            medium_width: HwpUnit::from_mm(120.0).unwrap(),
            medium_height: HwpUnit::from_mm(65.0).unwrap(),
        }
    }
}

fn make_chart(
    chart_type: ChartType,
    data: ChartData,
    width: HwpUnit,
    height: HwpUnit,
    title: &str,
    legend: LegendPosition,
) -> Control {
    Control::Chart {
        chart_type,
        data,
        width,
        height,
        title: Some(title.to_string()),
        legend,
        grouping: ChartGrouping::default(),
        bar_shape: None,
        explosion: None,
        of_pie_type: None,
        radar_style: None,
        wireframe: None,
        bubble_3d: None,
        scatter_style: None,
        show_markers: None,
        stock_variant: None,
    }
}

fn chart_paragraph(control: Control) -> Paragraph {
    Paragraph::with_runs(vec![Run::control(control, csi(CS_NORMAL))], psi(PS_CENTER))
}

fn push_chart_page_title(paras: &mut Vec<Paragraph>, title: &str) {
    paras.push(p(title, CS_TITLE, PS_LEFT));
    paras.push(empty());
}

fn push_chart_example(paras: &mut Vec<Paragraph>, label: Option<&str>, control: Control) {
    if let Some(label_text) = label {
        paras.push(p(label_text, CS_BOLD, PS_LEFT));
    }
    paras.push(chart_paragraph(control));
    paras.push(empty());
}

fn push_page_break(paras: &mut Vec<Paragraph>) {
    paras.push(empty().with_page_break());
}

fn report_header_row(cells: Vec<TableCell>) -> TableRow {
    TableRow::new(cells).with_header(true)
}

fn report_cell(text: &str, char_style: u32, para_style: u32, width_mm: f64) -> TableCell {
    TableCell::new(vec![p(text, char_style, para_style)], HwpUnit::from_mm(width_mm).unwrap())
}

fn report_table(rows: Vec<TableRow>, width_mm: f64, caption_text: &str) -> Table {
    Table::new(rows)
        .with_width(HwpUnit::from_mm(width_mm).unwrap())
        .with_caption(Caption::new(vec![p(caption_text, CS_SMALL, PS_CENTER)], CaptionSide::Bottom))
}

fn push_table(paras: &mut Vec<Paragraph>, table: Table) {
    paras.push(Paragraph::with_runs(vec![Run::table(table, csi(CS_NORMAL))], psi(PS_CENTER)));
    paras.push(empty());
}

fn append_column_pages(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    push_chart_page_title(paras, "[세로 막대 차트]");
    push_chart_example(
        paras,
        Some("1. Column — Clustered (기본):"),
        make_chart(
            ChartType::Column,
            ChartData::category(
                &["Q1", "Q2", "Q3", "Q4"],
                &[
                    ("매출", &[120.0, 180.0, 210.0, 250.0]),
                    ("비용", &[90.0, 110.0, 130.0, 160.0]),
                    ("이익", &[30.0, 70.0, 80.0, 90.0]),
                ],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "분기별 매출/비용/이익 (억원)",
            LegendPosition::Bottom,
        ),
    );
    push_chart_example(
        paras,
        Some("2. Column — Stacked:"),
        Control::Chart {
            chart_type: ChartType::Column,
            data: ChartData::category(
                &["2022", "2023", "2024", "2025"],
                &[
                    ("국내", &[450.0, 520.0, 580.0, 640.0]),
                    ("아시아", &[180.0, 230.0, 310.0, 380.0]),
                    ("유럽", &[90.0, 120.0, 160.0, 200.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("연도별 지역 매출 구성".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::Stacked,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[3D 세로 막대 + 100% 누적]");
    push_chart_example(
        paras,
        Some("3. Column3D — Cylinder:"),
        Control::Chart {
            chart_type: ChartType::Column3D,
            data: ChartData::category(
                &["서울", "경기", "부산", "대전", "광주"],
                &[
                    ("주거용", &[85.0, 72.0, 45.0, 28.0, 22.0]),
                    ("상업용", &[42.0, 38.0, 25.0, 15.0, 12.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("지역별 건축 허가 (백건)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: Some(BarShape::Cylinder),
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_chart_example(
        paras,
        Some("4. Column — PercentStacked:"),
        Control::Chart {
            chart_type: ChartType::Column,
            data: ChartData::category(
                &["10대", "20대", "30대", "40대", "50대+"],
                &[
                    ("모바일", &[95.0, 88.0, 75.0, 60.0, 40.0]),
                    ("PC", &[3.0, 8.0, 20.0, 32.0, 45.0]),
                    ("태블릿", &[2.0, 4.0, 5.0, 8.0, 15.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("연령대별 기기 사용 비율 (%)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::PercentStacked,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);
}

fn append_bar_line_area_pages(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    push_chart_page_title(paras, "[가로 막대 차트]");
    push_chart_example(
        paras,
        Some("5. Bar — 프로그래밍 언어 인기도:"),
        make_chart(
            ChartType::Bar,
            ChartData::category(
                &["Python", "JavaScript", "Java", "C++", "Rust", "Go"],
                &[("점유율(%)", &[28.0, 22.0, 16.0, 12.0, 8.0, 6.0])],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "2025 프로그래밍 언어 인기도",
            LegendPosition::None,
        ),
    );
    push_chart_example(
        paras,
        Some("6. Bar3D — Pyramid:"),
        Control::Chart {
            chart_type: ChartType::Bar3D,
            data: ChartData::category(
                &["전자", "자동차", "반도체", "조선", "바이오"],
                &[
                    ("수출(조원)", &[180.0, 95.0, 130.0, 42.0, 28.0]),
                    ("수입(조원)", &[60.0, 35.0, 80.0, 15.0, 22.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("산업별 수출입 (2025)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: Some(BarShape::Pyramid),
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[선형 차트]");
    push_chart_example(
        paras,
        Some("7. Line — 월별 기온 변화 (마커):"),
        Control::Chart {
            chart_type: ChartType::Line,
            data: ChartData::category(
                &["1월", "3월", "5월", "7월", "9월", "11월"],
                &[
                    ("서울", &[-2.4, 5.7, 18.6, 25.7, 21.2, 5.2]),
                    ("부산", &[3.1, 8.9, 18.1, 25.0, 22.5, 9.8]),
                    ("제주", &[5.8, 10.2, 18.5, 26.8, 23.1, 11.5]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("월별 평균 기온 (°C)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: Some(true),
            stock_variant: None,
        },
    );
    push_chart_example(
        paras,
        Some("8. Line3D:"),
        make_chart(
            ChartType::Line3D,
            ChartData::category(
                &["2021", "2022", "2023", "2024"],
                &[
                    ("회원수(만)", &[120.0, 185.0, 260.0, 340.0]),
                    ("MAU(만)", &[45.0, 92.0, 150.0, 220.0]),
                ],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "서비스 성장 추이",
            LegendPosition::Bottom,
        ),
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[영역 차트]");
    push_chart_example(
        paras,
        Some("9. Area — Stacked 트래픽:"),
        Control::Chart {
            chart_type: ChartType::Area,
            data: ChartData::category(
                &["00시", "04시", "08시", "12시", "16시", "20시"],
                &[
                    ("웹", &[120.0, 40.0, 380.0, 520.0, 490.0, 350.0]),
                    ("앱", &[80.0, 25.0, 250.0, 410.0, 380.0, 290.0]),
                    ("API", &[200.0, 180.0, 450.0, 600.0, 550.0, 400.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("시간대별 서버 트래픽 (req/s)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::Stacked,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_chart_example(
        paras,
        Some("10. Area3D — 에너지원별 발전량:"),
        make_chart(
            ChartType::Area3D,
            ChartData::category(
                &["2020", "2022", "2024", "2026E"],
                &[
                    ("원자력", &[160.0, 175.0, 190.0, 200.0]),
                    ("태양광", &[20.0, 45.0, 80.0, 120.0]),
                    ("풍력", &[10.0, 25.0, 50.0, 85.0]),
                ],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "에너지원별 발전량 (TWh)",
            LegendPosition::Bottom,
        ),
    );
    push_page_break(paras);
}

fn append_pie_scatter_and_radar_pages(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    push_chart_page_title(paras, "[원형 차트]");
    push_chart_example(
        paras,
        Some("11. Pie — 부서별 예산:"),
        make_chart(
            ChartType::Pie,
            ChartData::category(
                &["영업", "개발", "마케팅", "인사", "총무"],
                &[("예산(억)", &[35.0, 42.0, 18.0, 12.0, 8.0])],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "부서별 예산 배분",
            LegendPosition::Right,
        ),
    );
    push_chart_example(
        paras,
        Some("12. Pie3D — 시장 점유율:"),
        make_chart(
            ChartType::Pie3D,
            ChartData::category(
                &["삼성", "애플", "샤오미", "오포", "기타"],
                &[("점유율(%)", &[20.0, 27.0, 14.0, 9.0, 30.0])],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "글로벌 스마트폰 시장 점유율 (2025)",
            LegendPosition::Right,
        ),
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[도넛 / 원형 분리 차트]");
    push_chart_example(
        paras,
        Some("13. Doughnut — OS 점유율:"),
        make_chart(
            ChartType::Doughnut,
            ChartData::category(
                &["Windows", "macOS", "Linux", "ChromeOS"],
                &[("점유율(%)", &[72.0, 16.0, 8.0, 4.0])],
            ),
            sizes.medium_width,
            sizes.medium_height,
            "데스크톱 OS 점유율",
            LegendPosition::Right,
        ),
    );
    push_chart_example(
        paras,
        Some("14. OfPie (Pie-of-Pie) — 기타 항목 분리:"),
        Control::Chart {
            chart_type: ChartType::OfPie,
            data: ChartData::category(
                &["급여", "임대료", "마케팅", "서버비", "출장비", "교육비", "복리후생"],
                &[("비용(만원)", &[4500.0, 1200.0, 800.0, 600.0, 300.0, 200.0, 150.0])],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("운영 비용 구성 (기타 분리)".to_string()),
            legend: LegendPosition::Right,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: Some(OfPieType::Pie),
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[산점도]");
    push_chart_example(
        paras,
        Some("15. Scatter — Dots (점만):"),
        Control::Chart {
            chart_type: ChartType::Scatter,
            data: ChartData::xy(&[
                ("실험 A", &[1.0, 2.5, 3.2, 4.8, 6.0, 7.5], &[2.1, 5.2, 6.8, 9.5, 12.3, 15.0]),
                ("실험 B", &[1.0, 2.0, 3.5, 5.0, 6.5, 8.0], &[1.5, 3.8, 7.2, 10.1, 13.0, 16.8]),
            ]),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("실험 데이터 비교".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: Some(ScatterStyle::Dots),
            show_markers: None,
            stock_variant: None,
        },
    );
    push_chart_example(
        paras,
        Some("16. Scatter — SmoothMarker (곡선+마커):"),
        Control::Chart {
            chart_type: ChartType::Scatter,
            data: ChartData::xy(&[(
                "sin(x)",
                &[
                    0.0,
                    0.5,
                    1.0,
                    1.5,
                    2.0,
                    2.5,
                    3.0,
                    3.5,
                    4.0,
                    4.5,
                    5.0,
                    5.5,
                    std::f64::consts::TAU,
                ],
                &[0.0, 0.48, 0.84, 1.0, 0.91, 0.60, 0.14, -0.35, -0.76, -0.98, -0.96, -0.71, 0.0],
            )]),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("사인 함수 곡선".to_string()),
            legend: LegendPosition::None,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: Some(ScatterStyle::SmoothMarker),
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[버블 / 레이더 차트]");
    push_chart_example(
        paras,
        Some("17. Bubble — 도시별 비교:"),
        make_chart(
            ChartType::Bubble,
            ChartData::xy(&[
                ("서울", &[970.0], &[4200.0]),
                ("부산", &[340.0], &[3100.0]),
                ("인천", &[295.0], &[3300.0]),
                ("대구", &[240.0], &[2900.0]),
                ("대전", &[150.0], &[3000.0]),
            ]),
            sizes.medium_width,
            sizes.medium_height,
            "주요 도시 비교 (X=인구, Y=소득)",
            LegendPosition::Bottom,
        ),
    );
    push_chart_example(
        paras,
        Some("18. Radar — Filled (역량 평가):"),
        Control::Chart {
            chart_type: ChartType::Radar,
            data: ChartData::category(
                &["기술력", "커뮤니케이션", "리더십", "문제해결", "협업", "창의성"],
                &[
                    ("김철수", &[9.0, 7.0, 8.0, 9.0, 6.0, 8.0]),
                    ("이영희", &[7.0, 9.0, 7.0, 6.0, 9.0, 7.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("팀원 역량 비교".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: Some(RadarStyle::Filled),
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);

    push_chart_page_title(paras, "[레이더(마커) / 주식 차트]");
    push_chart_example(
        paras,
        Some("19. Radar — Marker (제품 비교):"),
        Control::Chart {
            chart_type: ChartType::Radar,
            data: ChartData::category(
                &["디자인", "성능", "배터리", "카메라", "가격"],
                &[
                    ("제품 A", &[8.0, 9.0, 7.0, 8.0, 6.0]),
                    ("제품 B", &[7.0, 7.0, 9.0, 6.0, 9.0]),
                    ("제품 C", &[9.0, 6.0, 6.0, 9.0, 7.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("스마트폰 비교 평가".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: Some(RadarStyle::Marker),
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_chart_example(
        paras,
        Some("20. Stock — OHLC (시가-고가-저가-종가):"),
        Control::Chart {
            chart_type: ChartType::Stock,
            data: ChartData::category(
                &["3/3", "3/4", "3/5", "3/6", "3/7"],
                &[
                    ("시가", &[52000.0, 52500.0, 53000.0, 51500.0, 52800.0]),
                    ("고가", &[53500.0, 54000.0, 53800.0, 53000.0, 54200.0]),
                    ("저가", &[51000.0, 51800.0, 51500.0, 50500.0, 52000.0]),
                    ("종가", &[52500.0, 53000.0, 51500.0, 52800.0, 53500.0]),
                ],
            ),
            width: sizes.large_width,
            height: sizes.large_height,
            title: Some("HWP전자 주가 (OHLC)".to_string()),
            legend: LegendPosition::None,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: Some(StockVariant::Ohlc),
        },
    );
}

fn append_report_trends(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    paras.push(p("1. 개요", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "2022년 ChatGPT 출시 이후 생성형 AI(Generative AI)의 급격한 발전은 \
         소프트웨어 개발 산업 전반에 구조적 변화를 가져왔다. \
         AI 코딩 도구(GitHub Copilot, Cursor 등)의 보편화로 개발 생산성이 \
         크게 향상된 반면, 신입 개발자의 채용 시장은 긴축 기조를 보이고 있다. \
         본 보고서는 2020~2025년 데이터를 바탕으로 AI가 \
         컴퓨터공학 전공자의 취업률에 미치는 영향을 분석한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    paras.push(p("2. 연도별 CS 전공자 취업률 추이", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "한국교육개발원 취업통계, Stanford AI Index 2025, \
         뉴욕 연방준비은행(Federal Reserve Bank of New York) 데이터를 종합한 \
         연도별 주요 지표는 다음과 같다. 2025년 기준 미국 CS 졸업생 실업률은 \
         6.1%로 전체 전공 중 7번째로 높으며, 한국 주요 대학 CS 취업률도 \
         서울대 83.8%(2023)→72.6%(2025) 등 큰 폭으로 하락하고 있다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    let trend_table: Table = report_table(
        vec![
            report_header_row(vec![
                report_cell("연도", CS_BOLD, PS_CENTER, 28.0),
                report_cell("취업률(%)", CS_BOLD, PS_CENTER, 28.0),
                report_cell("AI직무 비중(%)", CS_BOLD, PS_CENTER, 28.0),
                report_cell("평균연봉(만원)", CS_BOLD, PS_CENTER, 28.0),
                report_cell("채용공고수(만)", CS_BOLD, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2020", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("67.2", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("8.5", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("3,850", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("12.3", CS_NORMAL, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2021", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("69.8", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("11.2", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("4,200", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("15.7", CS_NORMAL, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2022", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("72.5", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("15.8", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("4,680", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("18.2", CS_NORMAL, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2023", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("68.1", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("22.4", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("4,950", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("14.5", CS_NORMAL, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2024", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("64.3", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("31.6", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("5,120", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("11.8", CS_NORMAL, PS_CENTER, 28.0),
            ]),
            TableRow::new(vec![
                report_cell("2025", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("61.7", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("38.2", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("5,350", CS_NORMAL, PS_CENTER, 28.0),
                report_cell("10.2", CS_NORMAL, PS_CENTER, 28.0),
            ]),
        ],
        140.0,
        "[표 1] 연도별 CS 전공자 취업 지표",
    );
    push_table(paras, trend_table);
    push_chart_example(
        paras,
        None,
        Control::Chart {
            chart_type: ChartType::Line,
            data: ChartData::category(
                &["2020", "2021", "2022", "2023", "2024", "2025"],
                &[
                    ("취업률(%)", &[67.2, 69.8, 72.5, 68.1, 64.3, 61.7]),
                    ("AI직무 비중(%)", &[8.5, 11.2, 15.8, 22.4, 31.6, 38.2]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("취업률 vs AI직무 비중 추이".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: Some(true),
            stock_variant: None,
        },
    );
    paras.push(p(
        "2022년까지 상승 추세를 보이던 취업률은 2023년을 기점으로 하락 전환하였다. \
         Stanford Digital Economy Lab(2025.11)에 따르면, 22~25세 AI 노출 직군 \
         취업자는 2022년 정점 대비 20% 감소하였으며, 미국 엔트리레벨 테크 채용은 \
         2023→2024년 67% 급감하였다(Stanford). 한국에서도 SW 개발직 채용 공고 중 \
         신입 비율이 2022년 53.5%에서 2024년 37.4%로 16.1%p 감소하였다(한국노동연구원).",
        CS_NORMAL,
        PS_LEFT,
    ));
    push_page_break(paras);
}

fn append_report_job_market(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    paras.push(p("3. 직무별 채용 시장 분석", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "Veritone Q1 2025 분석에 따르면, AI/ML 엔지니어 채용은 전년 대비 \
         41.8% 증가하였고, 생성형 AI 기술을 명시한 채용 공고는 2023년 16,000건에서 \
         2024년 66,000건으로 4배 폭증하였다(Stanford AI Index/Lightcast). \
         반면 BLS(미 노동통계국)는 컴퓨터 프로그래머 고용이 향후 10년간 \
         10% 감소할 것으로 전망한다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    let job_table: Table = report_table(
        vec![
            report_header_row(vec![
                report_cell("직무", CS_BOLD, PS_CENTER, 23.0),
                report_cell("공고수", CS_BOLD, PS_CENTER, 23.0),
                report_cell("전년대비", CS_BOLD, PS_CENTER, 23.0),
                report_cell("평균연봉", CS_BOLD, PS_CENTER, 23.0),
                report_cell("경쟁률", CS_BOLD, PS_CENTER, 23.0),
                report_cell("요구경력", CS_BOLD, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("AI/ML 엔지니어", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("18,500", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("+42%", CS_BLUE, PS_CENTER, 23.0),
                report_cell("6,800만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("8.2:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("3년+", CS_NORMAL, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("데이터 엔지니어", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("12,300", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("+28%", CS_BLUE, PS_CENTER, 23.0),
                report_cell("5,900만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("6.5:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("2년+", CS_NORMAL, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("백엔드 개발", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("22,100", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("-12%", CS_RED, PS_CENTER, 23.0),
                report_cell("5,200만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("15.3:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("2년+", CS_NORMAL, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("프론트엔드", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("15,800", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("-18%", CS_RED, PS_CENTER, 23.0),
                report_cell("4,800만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("18.7:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("1년+", CS_NORMAL, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("DevOps/SRE", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("8,900", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("+15%", CS_BLUE, PS_CENTER, 23.0),
                report_cell("6,200만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("5.1:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("3년+", CS_NORMAL, PS_CENTER, 23.0),
            ]),
            TableRow::new(vec![
                report_cell("SI/SM", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("9,200", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("-25%", CS_RED, PS_CENTER, 23.0),
                report_cell("3,600만", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("4.2:1", CS_NORMAL, PS_CENTER, 23.0),
                report_cell("무관", CS_NORMAL, PS_CENTER, 23.0),
            ]),
        ],
        138.0,
        "[표 2] 2025년 IT 직무별 채용 현황",
    );
    push_table(paras, job_table);
    push_chart_example(
        paras,
        None,
        Control::Chart {
            chart_type: ChartType::Bar,
            data: ChartData::category(
                &["AI/ML", "데이터", "백엔드", "프론트", "DevOps", "SI/SM"],
                &[("전년대비(%)", &[42.0, 28.0, -12.0, -18.0, 15.0, -25.0])],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("직무별 채용 증감률 (전년대비 %)".to_string()),
            legend: LegendPosition::None,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);
}

fn append_report_ai_tools(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    paras.push(p("4. AI 도구 활용 역량과 채용 상관관계", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "Stack Overflow 2025 Developer Survey(49,000명+, 177개국)에 따르면, \
         개발자의 84%가 AI 도구를 사용하며 51%는 매일 사용한다. \
         PwC 2025 Global AI Jobs Barometer는 AI 스킬 보유자의 임금 프리미엄이 \
         56%에 달한다고 보고했다. 특히 Anthropic 내부 설문에서 \
         AI 활용 엔지니어의 생산성 향상은 50%로 나타났다(1년 전 20%에서 상승).",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    let ai_table: Table = report_table(
        vec![
            report_header_row(vec![
                report_cell("AI 활용 수준", CS_BOLD, PS_CENTER, 35.0),
                report_cell("면접 통과율", CS_BOLD, PS_CENTER, 35.0),
                report_cell("연봉 프리미엄", CS_BOLD, PS_CENTER, 35.0),
                report_cell("취업 소요기간", CS_BOLD, PS_CENTER, 35.0),
            ]),
            TableRow::new(vec![
                report_cell("미활용", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("22%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("기준", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("평균 5.2개월", CS_NORMAL, PS_CENTER, 35.0),
            ]),
            TableRow::new(vec![
                report_cell("기본 활용", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("31%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("+8%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("평균 3.8개월", CS_NORMAL, PS_CENTER, 35.0),
            ]),
            TableRow::new(vec![
                report_cell("적극 활용", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("40%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("+15%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("평균 2.5개월", CS_NORMAL, PS_CENTER, 35.0),
            ]),
            TableRow::new(vec![
                report_cell("AI 프로젝트 경험", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("52%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("+28%", CS_NORMAL, PS_CENTER, 35.0),
                report_cell("평균 1.8개월", CS_NORMAL, PS_CENTER, 35.0),
            ]),
        ],
        140.0,
        "[표 3] AI 도구 활용 수준별 취업 성과 (2025)",
    );
    push_table(paras, ai_table);
    push_chart_example(
        paras,
        None,
        Control::Chart {
            chart_type: ChartType::Radar,
            data: ChartData::category(
                &["알고리즘", "시스템설계", "AI/ML", "커뮤니케이션", "문제해결", "코딩테스트"],
                &[
                    ("2022 요구역량", &[9.0, 7.0, 4.0, 6.0, 8.0, 9.0]),
                    ("2025 요구역량", &[7.0, 8.0, 9.0, 8.0, 9.0, 6.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("채용 시 요구역량 변화 (2022 vs 2025)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::default(),
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: Some(RadarStyle::Filled),
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    push_page_break(paras);
}

fn append_report_conclusion(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    paras.push(p("5. 결론 및 시사점", CS_BOLD, PS_LEFT));
    paras.push(empty());
    paras.push(p(
        "WEF Future of Jobs Report 2025에 따르면, 2030년까지 1억 7,000만 개의 \
         신규 일자리가 창출되나 9,200만 개가 소멸하여 순 7,800만 개 증가가 예상된다. \
         그러나 Goldman Sachs(2025)는 AI 완전 채택 시 미국 고용의 6~7%가 위협받을 수 \
         있다고 경고한다. 특히 Anthropic CEO Dario Amodei는 5년 내 엔트리레벨 \
         화이트칼라 직무의 50%가 소멸할 수 있다고 전망하였다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());

    let summary_table: Table = report_table(
        vec![
            report_header_row(vec![
                report_cell("항목", CS_BOLD, PS_LEFT, 45.0),
                report_cell("시사점", CS_BOLD, PS_LEFT, 95.0),
            ]),
            TableRow::new(vec![
                report_cell("취업률 전망", CS_NORMAL, PS_LEFT, 45.0),
                report_cell(
                    "2026년 CS 전공 취업률 60% 전후 예상. AI 직무 제외 시 50%대 하락 가능성",
                    CS_NORMAL,
                    PS_LEFT,
                    95.0,
                ),
            ]),
            TableRow::new(vec![
                report_cell("필수 역량 변화", CS_NORMAL, PS_LEFT, 45.0),
                report_cell(
                    "코딩테스트 비중 감소, AI/ML 활용 능력 및 시스템 설계 역량 중요도 상승",
                    CS_NORMAL,
                    PS_LEFT,
                    95.0,
                ),
            ]),
            TableRow::new(vec![
                report_cell("연봉 양극화", CS_NORMAL, PS_LEFT, 45.0),
                report_cell(
                    "AI 직무 평균연봉 6,800만원 vs SI 직무 3,600만원 — 1.9배 격차 확대 추세",
                    CS_NORMAL,
                    PS_LEFT,
                    95.0,
                ),
            ]),
            TableRow::new(vec![
                report_cell("교육과정 대응", CS_NORMAL, PS_LEFT, 45.0),
                report_cell(
                    "대학 교육과정에 AI/ML 필수화 시급. 현재 상위 20개교 중 AI 트랙 운영 비율 75%",
                    CS_NORMAL,
                    PS_LEFT,
                    95.0,
                ),
            ]),
        ],
        140.0,
        "[표 4] 주요 시사점 요약",
    );
    push_table(paras, summary_table);
    push_chart_example(
        paras,
        None,
        Control::Chart {
            chart_type: ChartType::Area,
            data: ChartData::category(
                &["2023", "2024", "2025", "2026E", "2027E"],
                &[
                    ("AI/ML 직무", &[22.0, 32.0, 38.0, 45.0, 52.0]),
                    ("전통 개발 직무", &[55.0, 48.0, 42.0, 38.0, 33.0]),
                    ("기타 IT 직무", &[23.0, 20.0, 20.0, 17.0, 15.0]),
                ],
            ),
            width: sizes.medium_width,
            height: sizes.medium_height,
            title: Some("IT 채용 시장 직무 구성 전망 (%)".to_string()),
            legend: LegendPosition::Bottom,
            grouping: ChartGrouping::PercentStacked,
            bar_shape: None,
            explosion: None,
            of_pie_type: None,
            radar_style: None,
            wireframe: None,
            bubble_3d: None,
            scatter_style: None,
            show_markers: None,
            stock_variant: None,
        },
    );
    paras.push(p(
        "향후 AI 도구의 발전은 개발자의 역할을 '코드 작성자'에서 \
         'AI 오케스트레이터'로 전환시킬 것으로 전망된다. \
         컴퓨터공학 전공자에게는 기초 CS 역량(자료구조, 알고리즘, \
         운영체제)을 바탕으로 AI 시스템의 설계·평가·통합 능력을 \
         갖추는 것이 취업 경쟁력의 핵심이 될 것이다.",
        CS_NORMAL,
        PS_LEFT,
    ));
    paras.push(empty());
    paras.push(p(
        "※ 본 데이터는 한국교육개발원, GitHub Developer Survey 2025, \
         Stack Overflow Annual Survey, 사람인/원티드 채용 데이터를 종합 분석한 것임.",
        CS_SMALL,
        PS_LEFT,
    ));
}

fn append_chart_showcase(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    append_column_pages(paras, sizes);
    append_bar_line_area_pages(paras, sizes);
    append_pie_scatter_and_radar_pages(paras, sizes);
}

fn append_employment_report(paras: &mut Vec<Paragraph>, sizes: ChartSizes) {
    push_page_break(paras);
    paras.push(p("AI 시대의 컴퓨터공학 전공자 취업 동향 분석", CS_TITLE, PS_CENTER));
    paras.push(p("2026년 3월 보고서", CS_SMALL, PS_CENTER));
    paras.push(empty());

    append_report_trends(paras, sizes);
    append_report_job_market(paras, sizes);
    append_report_ai_tools(paras, sizes);
    append_report_conclusion(paras, sizes);
}

pub(crate) fn gen_14_chart() {
    let store = showcase_store();
    let (mut paras, images) = mascot_intro(
        "14. 차트",
        "OOXML 차트 18종을 다양한 데이터와 옵션으로 시연합니다. \
         각 페이지에 1~2개 차트를 배치하여 레이아웃을 정리했습니다.",
    );
    let sizes: ChartSizes = ChartSizes::new();

    append_chart_showcase(&mut paras, sizes);
    append_employment_report(&mut paras, sizes);

    let mut section = Section::with_paragraphs(paras, PageSettings::a4());
    section.header = Some(HeaderFooter::new(
        vec![p("14. 차트 쇼케이스 — HwpForge", CS_SMALL, PS_LEFT)],
        ApplyPageType::Both,
    ));
    section.footer = Some(HeaderFooter::new(
        vec![p("생성일: 2026-03-08", CS_SMALL, PS_RIGHT)],
        ApplyPageType::Both,
    ));
    section.page_number =
        Some(PageNumber::new(PageNumberPosition::BottomCenter, NumberFormatType::Digit));

    let mut doc = Document::new();
    doc.add_section(section);
    encode_and_save("14_chart.hwpx", &store, &doc, &images);
}
