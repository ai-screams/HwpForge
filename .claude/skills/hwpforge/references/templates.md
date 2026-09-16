# Style Templates (Presets)

A preset is a built-in style set (fonts, sizes, page setup). `hwpforge templates list`
catalogs four, but **CLI `convert --preset` currently resolves only `default`** — the others
return `UNKNOWN_PRESET` from CLI `convert` today (catalogued/inspectable, not yet selectable
there). MCP `hwpforge_convert` and `hwpforge_restyle` accept all four.

## Available Presets

| Preset    | 본문 글꼴       | 용지 | CLI `convert --preset` | 설명                    |
| --------- | --------------- | ---- | ---------------------- | ----------------------- |
| `default` | 함초롬돋움 10pt | A4   | ✅ 사용 가능           | 한컴 Modern 기본 스타일 |
| `modern`  | 맑은 고딕       | A4   | ❌ UNKNOWN_PRESET      | 깔끔한 현대적 스타일    |
| `classic` | 바탕            | A4   | ❌ UNKNOWN_PRESET      | 전통적 문서 스타일      |
| `latest`  | 함초롬바탕 10pt | A4   | ❌ UNKNOWN_PRESET      | 최신 한컴 스타일        |

> For CLI `convert`, use `default`. The other three appear in `templates list`/`show` and work
> through MCP `hwpforge_convert`/`hwpforge_restyle`, but CLI `convert` does not wire them in yet
> — do not pass them to `--preset` there (it will error).

## Commands

```bash
hwpforge templates list            # human-readable list
hwpforge templates list --json     # machine-readable
hwpforge templates show default    # one preset's details
hwpforge templates show modern --json
```

## Using a preset

Only the CLI flag selects a preset for CLI `convert` — frontmatter has no `preset` field (unsupported keys such as `preset` are parsed and silently ignored; see markdown-guide.md). Use `default` — it is the only value CLI `convert` resolves today (MCP `hwpforge_convert` accepts all four):

```bash
hwpforge convert input.md -o output.hwpx --preset default
```

> Presets set styles for **new** documents created with `convert`. When editing an existing
> document via JSON round-trip, the document keeps its own styles — do not expect a preset to
> restyle an existing file. See [editing-workflow.md](editing-workflow.md).
