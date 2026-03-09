# Style Templates (Presets)

## Available Presets

| Preset       | Use Case    | 본문 글꼴   | 제목 글꼴        | 줄간격 |
| ------------ | ----------- | ----------- | ---------------- | ------ |
| `default`    | 범용 문서   | 바탕체 10pt | 고딕체 14pt Bold | 160%   |
| `government` | 정부 제안서 | 바탕체 11pt | 고딕체 16pt Bold | 160%   |
| `report`     | 연구 보고서 | 바탕체 10pt | 고딕체 14pt Bold | 170%   |
| `official`   | 공문서      | 바탕체 12pt | 고딕체 14pt Bold | 160%   |

## Commands

### List presets

```bash
# Human-readable
hwpforge templates list

# JSON output
hwpforge templates list --json
```

### Show preset details

```bash
hwpforge templates show government
hwpforge templates show government --json
```

## Using Presets

### In CLI convert command

```bash
hwpforge convert input.md -o output.hwpx --preset government
```

### In YAML frontmatter

```yaml
---
title: "문서 제목"
preset: government
---
```

The `--preset` CLI flag takes precedence over the frontmatter `preset` field.

## Preset Details

### default

General-purpose template suitable for most documents.

- 본문: 바탕체 10pt, 줄간격 160%
- 제목: 고딕체 14pt Bold
- 여백: 위/아래 20mm, 좌/우 20mm
- 용지: A4 세로

### government

Optimized for Korean government RFP proposals and tenders.

- 본문: 바탕체 11pt, 줄간격 160%
- 제목: 고딕체 16pt Bold
- 여백: 위/아래 20mm, 좌/우 25mm
- 용지: A4 세로
- 특징: 여백 넓음, 본문 글자 크기 큼

### report

Designed for research reports and analysis documents.

- 본문: 바탕체 10pt, 줄간격 170%
- 제목: 고딕체 14pt Bold
- 여백: 위/아래 25mm, 좌/우 30mm
- 쪽번호: 하단 중앙
- 용지: A4 세로
- 특징: 줄간격 넓음, 여백 넓음 (가독성 중심)

### official

For administrative correspondence and formal notices.

- 본문: 바탕체 12pt, 줄간격 160%
- 제목: 고딕체 14pt Bold
- 여백: 위 20mm, 아래 15mm, 좌/우 20mm
- 용지: A4 세로
- 특징: 본문 글자 크기 큼, 하단 여백 좁음

## Choosing a Preset

| Document Type             | Recommended Preset |
| ------------------------- | ------------------ |
| 정부 제안서 / RFP 대응    | `government`       |
| 연구 보고서 / 분석 보고서 | `report`           |
| 공문서 / 안내문 / 협조전  | `official`         |
| 기타 일반 문서            | `default`          |
