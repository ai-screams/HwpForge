# Examples

HwpForge 사용 예제 모음

## 생성된 HWPX 파일

다운로드하여 한글에서 열어보세요. 모두 HwpForge API로 생성된 파일입니다.

### 종합 가이드

| 파일                                                   | 설명                                            | 생성 코드                                           |
| ------------------------------------------------------ | ----------------------------------------------- | --------------------------------------------------- |
| [`hwpx_complete_guide.hwpx`](hwpx_complete_guide.hwpx) | 전체 API 데모 (4섹션, 표/이미지/차트/수식/도형) | [`hwpx_complete_guide.rs`][src-hwpx-complete-guide] |

### 기능별 개별 파일

[`feature_isolation.rs`][src-feature-isolation]로 생성. 각 파일은 하나의 기능을 다양한 옵션으로 시연합니다.

| 파일                                                   | 설명                                                                       |
| ------------------------------------------------------ | -------------------------------------------------------------------------- |
| [`01_text.hwpx`](01_text.hwpx)                         | 텍스트 — 정렬(좌/중/우/양쪽), 줄간격, 들여쓰기                             |
| [`02_rich_text.hwpx`](02_rich_text.hwpx)               | 서식 텍스트 — 볼드/이탤릭/밑줄, 글꼴 크기, 색상                            |
| [`03_table.hwpx`](03_table.hwpx)                       | 표 — 다양한 크기, 셀 배경색, 병합, 캡션                                    |
| [`04_header_footer.hwpx`](04_header_footer.hwpx)       | 머리글/바닥글 — 좌/중/우 배치, 페이지 번호                                 |
| [`05_footnote_endnote.hwpx`](05_footnote_endnote.hwpx) | 각주/미주 — 인라인 삽입, 다중 참조                                         |
| [`06_textbox.hwpx`](06_textbox.hwpx)                   | 글상자 — 크기/위치 변형, DropCapStyle, 캡션                                |
| [`07_line.hwpx`](07_line.hwpx)                         | 선 — 두께, 색상, 화살표 스타일 (Arrow/Spear/Diamond)                       |
| [`08_ellipse.hwpx`](08_ellipse.hwpx)                   | 타원 — 크기 변형, 채우기 (Solid/Gradient), 회전                            |
| [`09_polygon.hwpx`](09_polygon.hwpx)                   | 다각형 — 삼각형/마름모/오각형/화살표, 채우기                               |
| [`10_multi_column.hwpx`](10_multi_column.hwpx)         | 다단 — 2단/3단, 균등/비균등 배분, 구분선                                   |
| [`11_image.hwpx`](11_image.hwpx)                       | 이미지 — 파일 삽입, 크기 조절, 캡션                                        |
| [`12_hyperlink.hwpx`](12_hyperlink.hwpx)               | 하이퍼링크 — URL/메일, fieldBegin/End 패턴                                 |
| [`13_equation.hwpx`](13_equation.hwpx)                 | 수식 — 분수/근호/적분/행렬/삼각함수 (HancomEQN)                            |
| [`14_chart.hwpx`](14_chart.hwpx)                       | 차트 — Bar/Line/Pie/Area/Scatter/Doughnut/Radar/Bubble/Stock               |
| [`15_shapes_advanced.hwpx`](15_shapes_advanced.hwpx)   | 고급 도형 — Arc/Curve/ConnectLine, Fill(Solid/Gradient/Pattern), 회전/반전 |

[src-hwpx-complete-guide]: ../crates/hwpforge-smithy-hwpx/examples/hwpx_complete_guide.rs
[src-feature-isolation]: ../crates/hwpforge-smithy-hwpx/examples/feature_isolation.rs

## Example 소스 코드

소스: [`crates/hwpforge-smithy-hwpx/examples/`](../crates/hwpforge-smithy-hwpx/examples/)

### 쇼케이스

| 파일                                                | 설명                                                 |
| --------------------------------------------------- | ---------------------------------------------------- |
| [`hwpx_complete_guide.rs`][src-hwpx-complete-guide] | HWPX 문서 구조 완전 가이드 (4섹션, 전체 API 시연)    |
| [`feature_isolation.rs`][src-feature-isolation]     | 기능별 개별 HWPX 생성 (15개)                         |
| [`showcase.rs`][src-showcase]                       | 13개 API 데모 (Table, Image, TextBox, 도형, 차트 등) |
| [`full_report.rs`][src-full-report]                 | HWPX 포맷 분석 보고서 (4섹션)                        |
| [`architecture_guide.rs`][src-architecture-guide]   | 아키텍처 가이드 + Write API 통합 검증 (28개 생성자)  |

[src-showcase]: ../crates/hwpforge-smithy-hwpx/examples/showcase.rs
[src-full-report]: ../crates/hwpforge-smithy-hwpx/examples/full_report.rs
[src-architecture-guide]: ../crates/hwpforge-smithy-hwpx/examples/architecture_guide.rs

### 기능별 데모

| 파일                                        | 설명                             |
| ------------------------------------------- | -------------------------------- |
| [`chart_styles.rs`][src-chart-styles]       | 18종 차트 타입 데모              |
| [`equation_styles.rs`][src-equation-styles] | 12종 수식 카테고리               |
| [`line_styles.rs`][src-line-styles]         | 10종 선 스타일 변형              |
| [`large_table.rs`][src-large-table]         | 대용량 표 (페이지 분할 레이아웃) |

[src-chart-styles]: ../crates/hwpforge-smithy-hwpx/examples/chart_styles.rs
[src-equation-styles]: ../crates/hwpforge-smithy-hwpx/examples/equation_styles.rs
[src-line-styles]: ../crates/hwpforge-smithy-hwpx/examples/line_styles.rs
[src-large-table]: ../crates/hwpforge-smithy-hwpx/examples/large_table.rs

### 기능 검증

| 파일                                                              | 설명                                                     |
| ----------------------------------------------------------------- | -------------------------------------------------------- |
| [`style_infrastructure.rs`][src-style-infrastructure]             | StyleIndex, Alignment, BorderFill, charPr/paraPr         |
| [`paragraph_and_annotations.rs`][src-paragraph-and-annotations]   | NumberingDef, TabDef, EmphasisType, Dutmal, Compose      |
| [`page_layout.rs`][src-page-layout]                               | Gutter, Visibility, LineNumberShape, MasterPage          |
| [`shapes_and_references.rs`][src-shapes-and-references]           | Arc, Curve, ConnectLine, Bookmark, CrossRef, Field, Memo |
| [`text_direction_and_dropcap.rs`][src-text-direction-and-dropcap] | TextDirection, DropCapStyle, page_break, char border     |
| [`positioning_and_toc.rs`][src-positioning-and-toc]               | 도형 위치 지정, 차트 세부 변형, TOC titleMark            |
| [`review_api.rs`][src-review-api]                                 | API 변경사항 검증 (ShapeStyle, LineStyle 등)             |
| [`roundtrip_save.rs`][src-roundtrip-save]                         | HWPX 읽기/쓰기 round-trip                                |

[src-style-infrastructure]: ../crates/hwpforge-smithy-hwpx/examples/style_infrastructure.rs
[src-paragraph-and-annotations]: ../crates/hwpforge-smithy-hwpx/examples/paragraph_and_annotations.rs
[src-page-layout]: ../crates/hwpforge-smithy-hwpx/examples/page_layout.rs
[src-shapes-and-references]: ../crates/hwpforge-smithy-hwpx/examples/shapes_and_references.rs
[src-text-direction-and-dropcap]: ../crates/hwpforge-smithy-hwpx/examples/text_direction_and_dropcap.rs
[src-positioning-and-toc]: ../crates/hwpforge-smithy-hwpx/examples/positioning_and_toc.rs
[src-review-api]: ../crates/hwpforge-smithy-hwpx/examples/review_api.rs
[src-roundtrip-save]: ../crates/hwpforge-smithy-hwpx/examples/roundtrip_save.rs

## 실행 방법

```bash
# 기능별 15개 파일 생성
cargo run -p hwpforge-smithy-hwpx --example feature_isolation

# 종합 가이드 생성
cargo run -p hwpforge-smithy-hwpx --example hwpx_complete_guide

# 기타 예제
cargo run -p hwpforge-smithy-hwpx --example chart_styles
cargo run -p hwpforge-smithy-hwpx --example large_table

# Markdown → HWPX
cargo run -p hwpforge-smithy-md --example gen_hwpx
```

모든 출력은 `temp/` 디렉토리에 생성됩니다.
