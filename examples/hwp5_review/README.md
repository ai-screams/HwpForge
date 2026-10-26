# HWP5 → HWPX conversion output

This directory contains HwpForge encoder output for inspecting supported
format features. For self-contained generation examples, see
[`../showcase/`](../showcase/) and `crates/hwpforge-smithy-hwpx/examples`.

To convert your own input:

```bash
hwpforge convert-hwp5 input.hwp -o output.hwpx
hwpforge inspect output.hwpx
```

## 변환 목록

### CharShape (Wave 1)

- `sample-char-underline-variants.hwpx` — 밑줄 5종
- `sample-char-strike-variants.hwpx` — 취소선 3종
- `sample-char-breakwordlatin-variants.hwpx` — 영문 줄바꿈
- `sample-text-char-runs-basic.hwpx`

### ParaShape (Wave 2)

- `sample-para-alignments-all.hwpx` — 정렬 6종
- `sample-para-line-spacing.hwpx` — 줄간격 3모드
- `sample-para-indent-variants.hwpx` — 들여쓰기 4종
- `sample-para-page-break.hwpx` — 페이지 나누기
- `sample-para-border-shading.hwpx` — 문단 테두리/배경

### List/Numbering (Wave 3)

- `sample-bullet-list.hwpx`, `sample-numbered-list*.hwpx`, `sample-outline-list.hwpx`
- `sample-checkable-bullet-*.hwpx` — 체크 가능 불릿
- `sample-mixed-lists-with-outline.hwpx`

### Field/Object (Wave 4)

- `sample-field-footnote.hwpx` — 각주 4개 (Wave 4b)
- `sample-field-hyperlink-*.hwpx`, `sample-field-bookmark-crossref-basic.hwpx`, `sample-field-bookmark-crossref-v2.hwpx`
- `sample-field-page-number-basic.hwpx`
- `rect_simple.hwpx` — `Control::Rect` (Wave 4a)
- `chart_01_single_column.hwpx`, `chart_02_single_pie.hwpx`, `chart_03_line_or_scatter.hwpx` — OOXML chart 통과 (Wave 4c)
- `mixed_01_image_and_chart_same_doc.hwpx` — 이미지 + 차트 혼합 (Wave 4c)

### Table / Tab / Text

- `sample-table-cell.hwpx`, `sample-tab.hwpx`, `sample-table-tab.hwpx`
- `sample-text-tab-linebreak-basic.hwpx`, `sample-empty.hwpx`, `sample-fwspace.hwpx`, `sample-fwspace-fixed.hwpx`

## 알려진 한계

- 이 변환은 의미(semantic) 보존이 목표이며 픽셀 단위 레이아웃 일치는 보장하지 않습니다.
- 각주 `instId`/`number` 속성은 생략됨 (의미적으로 동일, Wave 4b judgement call).
- numbering layout fidelity (`ParaHead.align`/`autoIndent`/`textOffset`)는 별도 hotfix track (잔재 R4).
