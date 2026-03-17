# Examples

HwpForge 사용 예제 모음

## 생성된 HWPX 파일

다운로드하여 한글에서 열어보세요. 모두 HwpForge API로 생성된 파일입니다.

### 종합

| 파일                                                   | 설명                                            | 생성 코드                                           |
| ------------------------------------------------------ | ----------------------------------------------- | --------------------------------------------------- |
| [`hwpx_complete_guide.hwpx`](hwpx_complete_guide.hwpx) | 전체 API 데모 (4섹션, 표/이미지/차트/수식/도형) | [`hwpx_complete_guide.rs`][src-hwpx-complete-guide] |
| [`full_report.hwpx`](full_report.hwpx)                 | HWPX 포맷 분석 보고서 (4섹션)                   | [`full_report.rs`][src-full-report]                 |

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
[src-full-report]: ../crates/hwpforge-smithy-hwpx/examples/full_report.rs
[src-feature-isolation]: ../crates/hwpforge-smithy-hwpx/examples/feature_isolation.rs

### HWPX ↔ JSON 변환 예제

[`hwpx_json_roundtrip.rs`][src-json-roundtrip]로 생성. HWPX와 JSON 간 round-trip을 시연합니다.

#### HWPX → JSON

| 파일                                                                       | 설명                                          |
| -------------------------------------------------------------------------- | --------------------------------------------- |
| [`hwpx2json/01_text.hwpx`](hwpx2json/01_text.hwpx)                         | 입력 — 텍스트 예제 원본                       |
| [`hwpx2json/01_text.json`](hwpx2json/01_text.json)                         | 출력 — JSON 변환 결과 (스타일 포함)           |
| [`hwpx2json/hwpx_complete_guide.hwpx`](hwpx2json/hwpx_complete_guide.hwpx) | 입력 — 종합 가이드 (4섹션, 표/차트/수식/도형) |
| [`hwpx2json/hwpx_complete_guide.json`](hwpx2json/hwpx_complete_guide.json) | 출력 — JSON 변환 결과 (스타일 포함)           |

#### JSON → HWPX

| 파일                                                                       | 설명                          |
| -------------------------------------------------------------------------- | ----------------------------- |
| [`json2hwpx/01_text.json`](json2hwpx/01_text.json)                         | 입력 — 위 JSON과 동일         |
| [`json2hwpx/01_text.hwpx`](json2hwpx/01_text.hwpx)                         | 출력 — JSON에서 재변환된 HWPX |
| [`json2hwpx/hwpx_complete_guide.json`](json2hwpx/hwpx_complete_guide.json) | 입력 — 종합 가이드 JSON       |
| [`json2hwpx/hwpx_complete_guide.hwpx`](json2hwpx/hwpx_complete_guide.hwpx) | 출력 — JSON에서 재변환된 HWPX |

[src-json-roundtrip]: ../crates/hwpforge-smithy-hwpx/examples/hwpx_json_roundtrip.rs

### HWPX → Markdown 변환 예제

[`hwpx_md_convert.rs`][src-md-convert]로 생성하거나, CLI `hwpforge to-md`로 변환합니다. 이미지가 포함된 문서는 `images/` 디렉토리에 자동 추출됩니다.

| 파일                                                                                       | 설명                                               |
| ------------------------------------------------------------------------------------------ | -------------------------------------------------- |
| [`hwpx2md/01_text.hwpx`](hwpx2md/01_text.hwpx)                                             | 입력 — 텍스트 예제 원본                            |
| [`hwpx2md/01_text.md`](hwpx2md/01_text.md)                                                 | 출력 — GFM Markdown                                |
| [`hwpx2md/hwpx_complete_guide.hwpx`](hwpx2md/hwpx_complete_guide.hwpx)                     | 입력 — 종합 가이드 (4섹션, 표/차트/수식/도형)      |
| [`hwpx2md/hwpx_complete_guide.md`](hwpx2md/hwpx_complete_guide.md)                         | 출력 — GFM Markdown (차트/수식/도형은 텍스트 추출) |
| [`hwpx2md/full_report.hwpx`](hwpx2md/full_report.hwpx)                                     | 입력 — HWPX 포맷 분석 보고서 (4섹션)               |
| [`hwpx2md/full_report.md`](hwpx2md/full_report.md)                                         | 출력 — GFM Markdown                                |
| [`hwpx2md/붙임4-1_신청용_연구개발계획서.hwpx`](hwpx2md/붙임4-1_신청용_연구개발계획서.hwpx) | 입력 — 정부 R&D 계획서 실무 문서                   |
| [`hwpx2md/붙임4-1_신청용_연구개발계획서.md`](hwpx2md/붙임4-1_신청용_연구개발계획서.md)     | 출력 — GFM Markdown                                |

[src-md-convert]: ../crates/hwpforge-smithy-md/examples/hwpx_md_convert.rs

### HWP5 → HWPX 변환 예제

CLI `hwpforge convert-hwp5`로 생성합니다. 입력 `.hwp`와 HwpForge가 생성한 출력 `.hwpx`를 짝으로 둡니다.

| 입력 파일                                                                                                | 출력 파일                                                                                                  | 설명                                           |
| -------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| [`hwp52hwpx/hwp5_01.hwp`](hwp52hwpx/hwp5_01.hwp)                                                         | [`hwp52hwpx/hwp5_01.hwpx`](hwp52hwpx/hwp5_01.hwpx)                                                         | 기본 변환 예제 — 간단한 HWP5 텍스트 문서       |
| [`hwp52hwpx/mixed_02a_header_image_footer_text.hwp`](hwp52hwpx/mixed_02a_header_image_footer_text.hwp)   | [`hwp52hwpx/mixed_02a_header_image_footer_text.hwpx`](hwp52hwpx/mixed_02a_header_image_footer_text.hwpx)   | 복합 예제 — 머리글/바닥글, 이미지, 일반 텍스트 |
| [`hwp52hwpx/table_20_real_world_ministry_style.hwp`](hwp52hwpx/table_20_real_world_ministry_style.hwp)   | [`hwp52hwpx/table_20_real_world_ministry_style.hwpx`](hwp52hwpx/table_20_real_world_ministry_style.hwpx)   | 실무형 표 예제 — 대표 기관 문서 스타일 표      |
| [`hwp52hwpx/table_20_real_world_ministry_stress.hwp`](hwp52hwpx/table_20_real_world_ministry_stress.hwp) | [`hwp52hwpx/table_20_real_world_ministry_stress.hwpx`](hwp52hwpx/table_20_real_world_ministry_stress.hwpx) | 스트레스 예제 — 복잡한 실문서 양식형 병합 표   |

## 실행 방법

```bash
# 기능별 15개 파일 생성
cargo run -p hwpforge-smithy-hwpx --example feature_isolation

# 종합 가이드 생성
cargo run -p hwpforge-smithy-hwpx --example hwpx_complete_guide

# 종합 보고서 생성
cargo run -p hwpforge-smithy-hwpx --example full_report

# HWPX ↔ JSON round-trip
cargo run -p hwpforge-smithy-hwpx --example hwpx_json_roundtrip

# HWPX → Markdown
cargo run -p hwpforge-smithy-md --example hwpx_md_convert

# Markdown → HWPX
cargo run -p hwpforge-smithy-md --example gen_hwpx

# HWP5 → HWPX
mkdir -p examples/hwp52hwpx
cargo run -p hwpforge-bindings-cli -- convert-hwp5 tests/fixtures/hwp5_01.hwp -o examples/hwp52hwpx/hwp5_01.hwpx
cargo run -p hwpforge-bindings-cli -- convert-hwp5 tests/fixtures/mixed_02a_header_image_footer_text.hwp -o examples/hwp52hwpx/mixed_02a_header_image_footer_text.hwpx
cargo run -p hwpforge-bindings-cli -- convert-hwp5 tests/fixtures/table_20_real_world_ministry_style.hwp -o examples/hwp52hwpx/table_20_real_world_ministry_style.hwpx
cargo run -p hwpforge-bindings-cli -- convert-hwp5 tests/fixtures/table_20_real_world_ministry_stress.hwp -o examples/hwp52hwpx/table_20_real_world_ministry_stress.hwpx
```

기능별/종합 가이드는 `temp/`에, round-trip 예제는 `examples/hwpx2json/`과 `examples/json2hwpx/`에, MD 변환은 `examples/hwpx2md/`에, HWP5 변환 예제는 `examples/hwp52hwpx/`에 생성됩니다.
