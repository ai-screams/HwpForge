# Style Presets

A preset is a built-in style set (fonts, sizes, page setup) for **new** documents. `hwpforge templates list` catalogs four, and every interface accepts all four. Any other name fails (`UNKNOWN_PRESET` in the CLI, `PRESET_NOT_FOUND` in Python).

| Preset    | 본문 글꼴       | 용지 | 설명                    |
| --------- | --------------- | ---- | ----------------------- |
| `default` | 함초롬돋움 10pt | A4   | 한컴 Modern 기본 스타일 |
| `modern`  | 맑은 고딕       | A4   | 깔끔한 현대적 스타일    |
| `classic` | 바탕            | A4   | 전통적 문서 스타일      |
| `latest`  | 함초롬바탕 10pt | A4   | 최신 한컴 스타일        |

```bash
hwpforge templates list [--json]
hwpforge templates show default [--json]
hwpforge convert input.md -o output.hwpx --preset modern
```

Only the `--preset` flag (MCP `hwpforge_convert` `preset`, Python `convert_md(preset=…)`) selects a preset. Frontmatter has no `preset` key — unknown keys are parsed and silently ignored ([markdown-guide.md](markdown-guide.md)). MCP also exposes each preset's definition as a resource, `hwpforge://templates/<name>`.

## Changing the preset of an existing file (`restyle`)

MCP `hwpforge_restyle` and Python `Document.restyle(preset=…)` exist; the CLI has no equivalent. What they do is narrow and lossy:

- They swap the document's **first font face** for the preset's font — nothing else (sizes, paragraph shapes and page setup stay).
- They re-encode the whole package like `from-json --base`: on a 한컴-saved file the result loses `Preview/*` and `META-INF/container.rdf`, and every line-layout cache, so it can no longer go through `to-pdf` until 한컴 re-saves it. In the measured case (`table_01_basic_2x2.hwpx`, 0.16.6) this happened with `warnings: []`.

Do not restyle a submitted government form. When editing an existing document through the JSON round-trip, the document keeps its own styles ([editing-workflow.md](editing-workflow.md)).
