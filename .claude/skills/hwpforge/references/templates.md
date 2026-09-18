# Style Templates (Presets)

A preset is a built-in style set (fonts, sizes, page setup). `hwpforge templates list`
catalogs four, and CLI `convert --preset` accepts all four, each applying its declared font. A
name outside this catalog returns `UNKNOWN_PRESET`. MCP `hwpforge_convert` and
`hwpforge_restyle` accept all four too.

## Available Presets

| Preset    | 본문 글꼴       | 용지 | CLI `convert --preset` | 설명                    |
| --------- | --------------- | ---- | ---------------------- | ----------------------- |
| `default` | 함초롬돋움 10pt | A4   | ✅ 사용 가능           | 한컴 Modern 기본 스타일 |
| `modern`  | 맑은 고딕       | A4   | ✅ 사용 가능           | 깔끔한 현대적 스타일    |
| `classic` | 바탕            | A4   | ✅ 사용 가능           | 전통적 문서 스타일      |
| `latest`  | 함초롬바탕 10pt | A4   | ✅ 사용 가능           | 최신 한컴 스타일        |

> All four presets work for CLI `convert` and MCP `hwpforge_convert`/`hwpforge_restyle` alike.
> A `--preset` value outside this catalog (typo, made-up name) errors with `UNKNOWN_PRESET`.

## Commands

```bash
hwpforge templates list            # human-readable list
hwpforge templates list --json     # machine-readable
hwpforge templates show default    # one preset's details
hwpforge templates show modern --json
```

## Using a preset

Only the CLI flag selects a preset for CLI `convert` — frontmatter has no `preset` field (unsupported keys such as `preset` are parsed and silently ignored; see markdown-guide.md). Any of the four catalogued names works:

```bash
hwpforge convert input.md -o output.hwpx --preset default
hwpforge convert input.md -o output.hwpx --preset modern
```

> Presets set styles for **new** documents created with `convert`. When editing an existing
> document via JSON round-trip, the document keeps its own styles — do not expect a preset to
> restyle an existing file. See [editing-workflow.md](editing-workflow.md).
