# 차트 생성

HwpForge는 OOXML 차트 형식(`xmlns:c`)을 사용해 18종의 차트를 HWPX 문서에 삽입할 수 있습니다.

## 지원 차트 종류 (18종)

| 변형                    | 설명                            |
| ----------------------- | ------------------------------- |
| `Bar`                   | 가로 막대 차트                  |
| `Column`                | 세로 막대 차트                  |
| `Bar3D` / `Column3D`    | 3D 막대/세로 막대               |
| `Line` / `Line3D`       | 꺾은선 / 3D 꺾은선              |
| `Pie` / `Pie3D`         | 원형 / 3D 원형                  |
| `Doughnut`              | 도넛 차트                       |
| `OfPie`                 | 원형-of-원형 / 막대-of-원형     |
| `Area` / `Area3D`       | 영역 / 3D 영역                  |
| `Scatter`               | 분산형 (XY)                     |
| `Bubble`                | 버블 차트                       |
| `Radar`                 | 방사형 차트                     |
| `Surface` / `Surface3D` | 표면 / 3D 표면                  |
| `Stock`                 | 주식 차트 (HLC/OHLC/VHLC/VOHLC) |

## Control::Chart 생성 방법

차트는 `Control::Chart` 변형으로 표현됩니다. `Run::control()`로 런에 삽입하고, 그 런을 문단에 넣습니다.

```rust,no_run
use hwpforge_core::control::Control;
use hwpforge_core::chart::{ChartType, ChartData, ChartGrouping, LegendPosition};
use hwpforge_foundation::HwpUnit;

let chart = Control::Chart {
    chart_type: ChartType::Column,
    data: ChartData::category(
        &["1월", "2월", "3월", "4월"],
        &[("매출", &[1200.0, 1500.0, 1350.0, 1800.0])],
    ),
    title: Some("월별 매출".to_string()),
    legend: LegendPosition::Bottom,
    grouping: ChartGrouping::Clustered,
    width: HwpUnit::from_mm(120.0).unwrap(),
    height: HwpUnit::from_mm(80.0).unwrap(),
};
```

## ChartData: Category vs Xy 방식

### Category 방식 (막대, 꺾은선, 원형, 영역, 방사형 등)

카테고리 레이블(X축)과 여러 시리즈로 구성됩니다. 대부분의 차트 종류에 사용합니다.

```rust,no_run
use hwpforge_core::chart::ChartData;

// 편의 생성자: cats 슬라이스 + (이름, 값 슬라이스) 튜플 배열
let data = ChartData::category(
    &["1분기", "2분기", "3분기", "4분기"],
    &[
        ("매출액", &[4200.0, 5100.0, 4800.0, 6200.0]),
        ("비용", &[3100.0, 3400.0, 3200.0, 3900.0]),
    ],
);
```

### Xy 방식 (분산형, 버블)

X값과 Y값 쌍으로 구성됩니다. 두 변수 간의 관계를 나타낼 때 사용합니다.

```rust,no_run
use hwpforge_core::chart::ChartData;

// (이름, x값 슬라이스, y값 슬라이스) 튜플 배열
let data = ChartData::xy(&[
    ("데이터셋 A", &[1.0, 2.0, 3.0, 4.0], &[2.1, 3.9, 6.2, 7.8]),
    ("데이터셋 B", &[1.0, 2.0, 3.0, 4.0], &[1.5, 3.0, 5.0, 6.5]),
]);
```

## ChartSeries, XySeries 구조

시리즈를 직접 구성할 때는 구조체를 사용합니다.

```rust,no_run
use hwpforge_core::chart::{ChartData, ChartSeries, XySeries};

// Category용 시리즈
let series = ChartSeries {
    name: "판매량".to_string(),
    values: vec![100.0, 150.0, 200.0],
};

let data = ChartData::Category {
    categories: vec!["A".to_string(), "B".to_string(), "C".to_string()],
    series: vec![series],
};

// XY용 시리즈
let xy_series = XySeries {
    name: "측정값".to_string(),
    x_values: vec![0.0, 1.0, 2.0],
    y_values: vec![0.0, 1.0, 4.0],
};
```

## 차트를 문단에 삽입하는 패턴

차트 `Control`을 `Run::control()`로 감싼 뒤, `Paragraph::with_runs()`에 포함시킵니다.

```rust,no_run
use hwpforge_core::control::Control;
use hwpforge_core::chart::{ChartType, ChartData, ChartGrouping, LegendPosition};
use hwpforge_core::run::Run;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, HwpUnit};

let chart_control = Control::Chart {
    chart_type: ChartType::Column,
    data: ChartData::category(
        &["A", "B", "C"],
        &[("값", &[10.0, 20.0, 30.0])],
    ),
    title: None,
    legend: LegendPosition::Right,
    grouping: ChartGrouping::Clustered,
    width: HwpUnit::from_mm(100.0).unwrap(),
    height: HwpUnit::from_mm(70.0).unwrap(),
};

let para = Paragraph::with_runs(
    vec![Run::control(chart_control, CharShapeIndex::new(0))],
    ParaShapeIndex::new(0),
);
```

## 예제: 막대 차트 (Column)

```rust,no_run
use hwpforge_core::control::Control;
use hwpforge_core::chart::{ChartType, ChartData, ChartGrouping, LegendPosition};
use hwpforge_core::run::Run;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::{Document, Section, PageSettings};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex, HwpUnit};

let data = ChartData::category(
    &["2022", "2023", "2024", "2025"],
    &[
        ("국내 매출", &[3200.0, 4100.0, 5300.0, 6800.0]),
        ("해외 매출", &[1100.0, 1800.0, 2700.0, 3900.0]),
    ],
);

let chart = Control::Chart {
    chart_type: ChartType::Column,
    data,
    title: Some("연도별 매출 현황 (단위: 백만원)".to_string()),
    legend: LegendPosition::Bottom,
    grouping: ChartGrouping::Clustered,
    width: HwpUnit::from_mm(140.0).unwrap(),
    height: HwpUnit::from_mm(90.0).unwrap(),
};

let mut doc = Document::new();
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::control(chart, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));

let validated = doc.validate().unwrap();
let bytes = HwpxEncoder::encode(
    &validated,
    &HwpxStyleStore::default_modern(),
    &Default::default(),
).unwrap();
std::fs::write("bar_chart.hwpx", &bytes).unwrap();
```

## 예제: 원형 차트 (Pie)

```rust,no_run
use hwpforge_core::control::Control;
use hwpforge_core::chart::{ChartType, ChartData, ChartGrouping, LegendPosition};
use hwpforge_foundation::{HwpUnit};

// 원형 차트는 단일 시리즈 사용
let chart = Control::Chart {
    chart_type: ChartType::Pie,
    data: ChartData::category(
        &["서울", "경기", "부산", "기타"],
        &[("비율", &[38.5, 25.2, 12.8, 23.5])],
    ),
    title: Some("지역별 매출 비중".to_string()),
    legend: LegendPosition::Right,
    grouping: ChartGrouping::Standard, // Pie는 Standard 사용
    width: HwpUnit::from_mm(100.0).unwrap(),
    height: HwpUnit::from_mm(80.0).unwrap(),
};
```

> **주의**: 차트 XML은 ZIP에 포함되지만 `content.hpf` 매니페스트에는 등록하지 않습니다. 매니페스트에 등록하면 한글이 크래시합니다.
