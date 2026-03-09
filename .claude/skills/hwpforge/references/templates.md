# Style Templates (Presets)

## Available Presets

| Preset       | Use Case    | 본문 글꼴       | Status    |
| ------------ | ----------- | --------------- | --------- |
| `default`    | 범용 문서   | 함초롬돋움 10pt | Available |
| `government` | 정부 제안서 | —               | Planned   |
| `report`     | 연구 보고서 | —               | Planned   |
| `official`   | 공문서      | —               | Planned   |

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
hwpforge templates show default
hwpforge templates show default --json
```

## Using Presets

### In CLI convert command

```bash
hwpforge convert input.md -o output.hwpx --preset default
```

### In YAML frontmatter

```yaml
---
title: "문서 제목"
preset: default
---
```

The `--preset` CLI flag takes precedence over the frontmatter `preset` field.

## Preset Details

### default (Available)

General-purpose template using 한컴 Modern style set.

- 본문: 함초롬돋움 10pt
- 용지: A4 세로

### government (Planned)

Optimized for Korean government RFP proposals and tenders.

### report (Planned)

Designed for research reports and analysis documents.

### official (Planned)

For administrative correspondence and formal notices.

## Choosing a Preset

| Document Type             | Recommended Preset     |
| ------------------------- | ---------------------- |
| 기타 일반 문서            | `default`              |
| 정부 제안서 / RFP 대응    | `government` (planned) |
| 연구 보고서 / 분석 보고서 | `report` (planned)     |
| 공문서 / 안내문 / 협조전  | `official` (planned)   |
