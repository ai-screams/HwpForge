---
name: hwpforge
description: "Generate, inspect, edit, and fill Korean HWP/HWPX documents using HwpForge. Use when the user asks to create a Korean government document, proposal, report, or official letter; convert Markdown to HWPX; convert an old HWP5 (.hwp) file to HWPX; fill in a Korean template (e.g. 국가과제 제안서) with content; edit/append content in an existing HWPX; inspect HWPX structure; or convert HWPX back to Markdown. Supports Markdown↔HWPX, HWP5→HWPX, JSON round-trip editing, template filling, and style presets."
license: MIT
compatibility: claude-code, openai-codex, cursor, windsurf, vscode-copilot
metadata:
  author: ai-screams
  version: "0.2.0"
allowed-tools: Bash Read Write
---

# HwpForge Skill

## Overview

HwpForge is a CLI (`hwpforge`) for the Korean HWPX document format (KS X 6101) used in
government proposals, official reports, and administrative documents. It can:

- **Create** HWPX from Markdown (with Korean style presets)
- **Convert** legacy HWP5 (`.hwp`) → HWPX, and HWPX → Markdown
- **Edit** existing HWPX via a JSON round-trip (fill placeholders, fix text, add content)
- **Inspect** structure and emit JSON Schemas

Every command accepts `--json` for machine-readable output and structured error codes.

> **There is no `.hwp` (HWP5) writer.** HwpForge reads `.hwp` but only writes `.hwpx`.
> To work with a `.hwp`, convert it to `.hwpx` first (`convert-hwp5`).

## The Algorithm — pick the right command

Follow this decision flow. Choosing the wrong path is the most common mistake.

```
What does the user want?
│
├─ Create a NEW document from text / Markdown
│     → convert  (Markdown → HWPX, with --preset)
│
├─ They have a legacy .hwp file
│     → convert-hwp5  (.hwp → .hwpx)   then treat it as HWPX
│
├─ EDIT an existing .hwpx  ── ALWAYS `inspect` first ──
│   │
│   ├─ Fill NAMED click-here fields (누름틀) — form-style templates
│   │     → fields (discover names) → fill --set name=value    [DELTA, cheapest+safest]
│   │
│   ├─ Template has NO 누름틀, only prose placeholders (□, (   ), 년 월 일, (인), @)
│   │     → stamp-plan (discover) → author spec map → stamp --map  [STAMP, one-time]
│   │       then the stamped output is a form template: use fields/fill above
│   │
│   ├─ Fill a TABLE CELL by position or label (병합셀 표 서식)
│   │     → to-json (cells carry addr {row,col}) → set-cell     [GRID, admission-gated]
│   │       --table N --at "r,c" | --right-of LABEL | --below LABEL, --text "" clears
│   │       covered coords resolve to their merge anchor (reported in the result)
│   │
│   ├─ Change only EXISTING text
│   │   (fill a prose placeholder, fix a typo, fill a table cell)
│   │     → to-json (--section) → edit the Text → patch        [TEXT-ONLY, safest]
│   │
│   └─ ADD or REMOVE paragraphs (structural change)
│         → to-json (full) → add paragraphs → from-json --base [REBUILD]
│
├─ Read / export an existing .hwpx
│     → to-md   (HWPX → Markdown, for reading)
│     → to-json (HWPX → JSON,    for machine editing)
│
└─ Need the JSON shape, or the list of styles
      → schema        (JSON Schema for document/section types)
      → templates list (available style presets)
```

**The two edit modes are not interchangeable:**

| Mode          | Command            | Can do                                                                                                       | Cannot do                                                                    |
| ------------- | ------------------ | ------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------- |
| **Text-only** | `patch`            | change text inside existing paragraphs **and table cells**; preserves images, styles, tables, layout exactly | add/remove paragraphs (returns `PATCH_FAILED: structural change detected`)   |
| **Rebuild**   | `from-json --base` | add/remove paragraphs, structural edits; preserves tables; `--base` inherits images                          | guarantee byte-perfect fidelity of complex 한컴 forms (see Fidelity Warning) |

## Commands

Run `hwpforge <command> --help` for exact flags. Key forms:

```bash
# Create  (convert --preset currently accepts only `default`; see Presets)
hwpforge convert input.md -o out.hwpx [--preset default]
echo "# 제목" | hwpforge convert - -o out.hwpx          # stdin via "-"

# Legacy HWP5
hwpforge convert-hwp5 old.hwp -o out.hwpx

# Inspect (ALWAYS before editing)
hwpforge inspect doc.hwpx [--styles] [--json]

# Named click-here fields (누름틀) — form templates: discover then fill
hwpforge fields doc.hwpx [--json]                        # list name/hint/current/fillable
hwpforge fill doc.hwpx --set 과제명="AI 문서 자동화" --set 기관명="AiScream" -o out.hwpx
#   all-or-nothing: 하나라도 검증 실패(없는 이름/중복 이름/빈 값/모호 필드)면 아무 것도 안 씀
#   나머지 패키지 엔트리는 바이트 그대로 보존 (preserve-first)

# Template stamping (E6) — promote prose placeholders (□, (   ), 년 월 일, (인), @)
# to named 누름틀 so fields/fill work. One-time preprocessing per template.
hwpforge stamp-plan template.hwpx --json                 # discover candidates
#   → author a spec map: EVERY unguarded candidate gets {"action":{"field":{"name":"…"}}}
#     or {"action":"ignore"}; guarded ones (※/【작성방법】/(예시) context) may be omitted
hwpforge stamp template.hwpx --map specs.json -o form.hwpx   # + form.manifest.json
#   fail-closed: 무손실 왕복이 증명 안 되는 입력은 거부(INPUT_NOT_ROUNDTRIP_SAFE);
#   무가드 후보 누락도 거부(STAMP_CANDIDATE_UNCOVERED). 이후 fields/fill 로 채움

# Grid cell editing (E3) — fill table cells by logical grid address.
# to-json export annotates every cell with addr {row,col} (병합 전 논리 격자).
hwpforge set-cell form.hwpx --table 0 --at "1,2" --text "홍길동" -o out.hwpx
hwpforge set-cell form.hwpx --table 0 --right-of "성명" --text "홍길동" -o out.hwpx
hwpforge set-cell form.hwpx --table 0 --below "비고" --text "" -o out.hwpx   # "" = clear
hwpforge set-cell form.hwpx --map cells.json -o out.hwpx   # batch: [{"table":0,"at":{"row":1,"col":2},"text":"…"}]
#   피병합 좌표는 병합 앵커로 resolve (결과에 requested/anchor/resolution 명시);
#   라벨은 NFC+공백 정규화 exact match (모호하면 CELL_LABEL_AMBIGUOUS — --at 로 지정);
#   표/이미지/컨트롤 든 셀은 거부(CELL_HAS_NON_TEXT_CONTENT); stamp 와 같은 admission 게이트

# Export  (NOTE: -o/--output is REQUIRED — there is no stdout export)
hwpforge to-json doc.hwpx -o full.json                  # whole document
hwpforge to-json doc.hwpx --section 0 -o sec.json       # one section
hwpforge to-json doc.hwpx --section 0 --no-styles -o sec.json

# Write back
hwpforge patch doc.hwpx --section 0 sec.json -o doc.hwpx          # text-only
hwpforge from-json full.json -o doc.hwpx --base doc.hwpx          # rebuild (inherit images)

# Read out / schema / styles
hwpforge to-md doc.hwpx -o doc.md
hwpforge schema [document|exported-document|exported-section]
hwpforge templates list [--json]
hwpforge templates show default
```

Diagnostic (parity/QA, not for normal authoring): `audit-hwp5`, `census-hwp5`.

## Presets

`templates list` catalogs four: `default` (함초롬돋움 10pt), `modern` (맑은 고딕),
`classic` (바탕), `latest` (함초롬바탕) — all A4. **However, `convert --preset` currently
resolves only `default`** (others return `UNKNOWN_PRESET`). Use `default` for `convert`; the
catalog entries are inspectable via `hwpforge templates show <name>`. See
[templates.md](references/templates.md).

## Editing an existing document (JSON round-trip)

The exported section/document JSON is structure + style **references** (IDs). Full recipes:
[editing-workflow.md](references/editing-workflow.md). Filling a Korean template (e.g. 국가과제
제안서): [template-fill.md](references/template-fill.md).

Minimal text-only edit (fill placeholders, fix text, fill table cells):

```bash
hwpforge inspect doc.hwpx --json                        # 1. understand structure
hwpforge to-json doc.hwpx --section 0 -o sec.json        # 2. export section
#   3. edit runs[].content.Text (and table cell text) in sec.json — keep style IDs as-is
hwpforge patch doc.hwpx --section 0 sec.json -o doc.hwpx  # 4. write back (text-only)
hwpforge inspect doc.hwpx                                 # 5. verify
```

Add new paragraphs (structural → rebuild):

```bash
hwpforge to-json doc.hwpx -o full.json                          # full document
#   append paragraph objects to document.sections[N].paragraphs
#   reuse a neighboring paragraph's para_shape_id + char_shape_id (do NOT invent IDs)
hwpforge from-json full.json -o doc.hwpx --base doc.hwpx        # rebuild
hwpforge inspect doc.hwpx                                       # paragraph count increased
```

### JSON rules (these prevent broken output)

- **Reuse existing style IDs.** New paragraphs/runs must copy `para_shape_id` / `char_shape_id`
  from a neighboring paragraph in the same document. Never invent IDs.
- `style_id` and `heading_level` are **optional** per paragraph — copy them only if the
  source paragraph has them; omit otherwise.
- **`patch` replaces the whole section**, so `sec.json` must contain ALL existing paragraphs
  plus your edits — it is a read-modify-write of the full section, not a delta.
- Do not edit the `styles` registry by hand — change styles via `--preset` instead.
- Table cell text lives at
  `…content.Table.rows[].cells[].paragraphs[].runs[].content.Text`.

## Fidelity Warning (government / 한컴-authored templates)

`patch` (text-only) preserves the original file structure exactly — **prefer it for real
한컴 templates** (form fields, master pages, complex tables) where formatting is mandatory.

`from-json --base` **rebuilds** the document from HwpForge's internal model. Simple tables and
paragraphs survive, but elements HwpForge does not yet fully model (form controls, master pages,
some advanced formatting) can be lost. **Never submit a rebuilt government document without
opening it in 한컴 and checking it visually.** When in doubt, fill placeholders with `patch`.

## Document Scenarios

| Scenario                          | File                                                    | Use When                                      |
| --------------------------------- | ------------------------------------------------------- | --------------------------------------------- |
| 정부 제안서 (Government Proposal) | [scenario-proposal.md](references/scenario-proposal.md) | RFP response, project bid, tender             |
| 보고서 (Report)                   | [scenario-report.md](references/scenario-report.md)     | Research/progress report, analysis            |
| 공문서 (Official Document)        | [scenario-official.md](references/scenario-official.md) | Administrative correspondence, notice         |
| 템플릿 채우기 (Template Fill)     | [template-fill.md](references/template-fill.md)         | Fill an existing Korean template with content |

## Korean Markdown Best Practices

See [markdown-guide.md](references/markdown-guide.md): GFM tables, YAML frontmatter
(`title`, `author`, `date`, `preset`), image paths, `---` as page break, Korean characters.

## Agent Behavior Rules

### Output: No Raw JSON

Never show raw JSON to the user during round-trip workflows. Summarize as a table, structure
diagram, or short description. Keep intermediate JSON in temp files for internal use only.

### Edit: In-Place by Default

When the user asks to modify a specific file, overwrite the original unless they specify a
different output path — set `-o` to the input path.

```bash
hwpforge patch document.hwpx --section 0 modified.json -o document.hwpx   # default: overwrite
```

### Always inspect before editing, always verify after

Run `inspect` first to learn the structure, and `inspect` (or `to-md`) after to confirm the
edit landed before reporting success.

## Error Handling

With `--json`, all commands return structured errors:

```json
{ "error": { "code": "PATCH_FAILED", "message": "...", "hint": "..." } }
```

Common: `FILE_NOT_FOUND` (bad path), `PATCH_FAILED` with "structural change detected"
(you added/removed paragraphs in a `patch` — use `from-json --base` instead).

Exit codes: `1` user error (bad input/missing file), `2` internal error (encode/corrupt).
Use `--json` in all agent workflows to parse errors programmatically.
