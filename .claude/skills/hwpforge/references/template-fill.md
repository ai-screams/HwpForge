# Filling a Korean Template (e.g. 국가과제 제안서)

A template is an existing `.hwpx` (or `.hwp`) with fixed sections, headings, tables and placeholders (`(작성)`, `(여기에 … 작성하시오)`, `○○○`, 누름틀). The goal is to put the user's content in **without changing anything else** — government submissions are checked for format. Every recipe writes to a new file and ends with the verify loop in SKILL.md.

## Pick the route

```bash
hwpforge outline template.hwpx --json     # headings, tables, fields
hwpforge fields template.hwpx --json      # any fillable 누름틀?
```

| The template has…                        | Route                                                   | Works on a 한컴-saved file?        |
| ---------------------------------------- | ------------------------------------------------------- | ---------------------------------- |
| fillable 누름틀                          | `fill --set name=value` (Recipe A)                      | yes                                |
| text or table-cell placeholders          | `to-json --section N` → edit → `patch` (Recipe B)       | yes                                |
| placeholders you want as reusable 누름틀 | `stamp` once, then `fill` ([stamp.md](stamp.md))        | no — refused                       |
| empty cells you address by grid position | `set-cell` ([structural-edit.md](structural-edit.md))   | no — refused                       |
| a heading that needs new body paragraphs | `from-json --base` (Recipe C), only on explicit request | lossy — drops 한컴 package entries |

Most real templates were saved by 한컴, so Recipes A and B are the normal path. The full surface table is in [editing-workflow.md](editing-workflow.md).

A legacy `.hwp` template must be converted first — there is no `.hwp` writer. The converted `.hwpx` has no 한컴 package entries, so every route above (including `set-cell` and `stamp`) accepts it:

```bash
hwpforge convert-hwp5 template.hwp -o template.hwpx
```

## Recipe A — fill 누름틀

```bash
hwpforge fill template.hwpx --set 과제명="AI 문서 자동화" --set 기관명="AiScream" -o 제안서_제출본.hwpx --json
hwpforge diff template.hwpx 제안서_제출본.hwpx --json
```

All-or-nothing: an unknown, duplicate or empty value fails the whole call and writes nothing. In the diff, `field_values` shows one `value_changed` per name, and each filled paragraph shows a `raw` entry `$.layout_cache` — expected.

## Recipe B — replace placeholder text (body and table cells)

```bash
hwpforge to-json template.hwpx --section 0 -o sec.json
#   edit Text values in sec.json (script below); keep every paragraph, keep `preservation`
hwpforge patch template.hwpx --section 0 sec.json -o 제안서_제출본.hwpx --json
hwpforge validate 제안서_제출본.hwpx --json
hwpforge diff template.hwpx 제안서_제출본.hwpx --json      # only the paragraphs/cells you filled
```

Body placeholders are usually unique, so a text → value map works. **Table placeholders repeat** (`(작성)` in every budget cell), so fill tables by **logical grid address**: find the row whose label sits in logical column 0, then set logical columns 1, 2, … from each cell's `addr` (array position differs from the grid when cells are merged). The script is all-or-nothing: every requested value must land exactly once, otherwise it writes nothing and lists why.

```python
import json
import sys

d = json.load(open("sec.json"))

BODY = {  # placeholder paragraph text → replacement
    "(연구개발 목표를 작성하시오)": "본 과제는 …를 목표로 한다.",
}
BUDGET = {  # 연구비 표: row label (logical column 0) → values for logical columns 1, 2, …
    "인건비": ["120,000", "130,000"],
    "재료비": ["30,000", "20,000"],
}

def cell_text(cell):  # all text of a cell
    return "\n".join("".join(r["content"].get("Text", "") for r in p["runs"]) for p in cell["paragraphs"])

def set_text(cell, value):
    # Only a cell with ONE paragraph holding ONE Text run is replaced; anything
    # richer (bold + plain runs, several lines, a control) would keep old text.
    paras = cell["paragraphs"]
    if len(paras) != 1 or len(paras[0]["runs"]) != 1 or "Text" not in paras[0]["runs"][0]["content"]:
        raise ValueError(f"cell {cell['addr']} is not a single plain run: {cell_text(cell)!r}")
    paras[0]["runs"][0]["content"]["Text"] = value

errors = []
body_hits = {k: 0 for k in BODY}
tables = []
for p in d["section"]["paragraphs"]:
    for r in p.get("runs", []):
        c = r.get("content", {})
        if c.get("Text") in BODY:
            body_hits[c["Text"]] += 1
            c["Text"] = BODY[c["Text"]]
        if "Table" in c:
            tables.append(c["Table"])
for key, n in body_hits.items():
    if n != 1:
        errors.append(f"body placeholder {key!r} found {n} times (need exactly 1)")

label_hits = {k: [] for k in BUDGET}
for ti, t in enumerate(tables):
    cells = [cell for row in t["rows"] for cell in row["cells"]]
    if any("addr" not in cell for cell in cells):
        errors.append(f"table {ti} has cells without addr — re-export with to-json")
        continue
    grid = {(cell["addr"]["row"], cell["addr"]["col"]): cell for cell in cells}  # logical grid, merged cells at their anchor
    for (row, col), cell in grid.items():
        if col == 0 and cell_text(cell) in BUDGET:
            label_hits[cell_text(cell)].append((ti, row, grid))
for label, hits in label_hits.items():
    if len(hits) != 1:
        errors.append(f"row label {label!r} found {len(hits)} times (need exactly 1)")
        continue
    ti, row, grid = hits[0]
    for col, value in enumerate(BUDGET[label], start=1):
        cell = grid.get((row, col))
        if cell is None:
            errors.append(f"{label!r}: table {ti} has no cell at logical row {row}, col {col}")
            continue
        try:
            set_text(cell, value)
        except ValueError as e:
            errors.append(f"{label!r} col {col}: {e}")

if errors:  # nothing is written — sec.json stays as exported
    sys.exit("not applied:\n  " + "\n  ".join(errors))
json.dump(d, open("sec.json", "w"), ensure_ascii=False)
print(f"applied {len(BODY)} body placeholder(s) and {sum(len(v) for v in BUDGET.values())} cell(s)")
```

Measured on 0.16.6 (tables converted from Markdown, then `to-json --section 0`):

| Input                                                           | Output                                                                                                                        | `sec.json`            |
| --------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | --------------------- |
| body placeholder + 인건비/재료비 rows with plain `(작성)` cells | `applied 1 body placeholder(s) and 4 cell(s)`; after `patch`, `diff` shows paragraph 1 and cells (1,1) (1,2) (2,1) (2,2) only | rewritten             |
| same, but 재료비 1년차 is `**(작성)** 원` (two runs)            | `not applied:` / `'재료비' col 1: cell {'row': 2, 'col': 1} is not a single plain run: '(작성) 원'`, exit 1                   | unchanged (same hash) |

On a merged table (label `비고` spanning rows 1–2), row 2 of the export holds a single cell whose `addr` is (2,1) at array index 0 — selecting by `addr` put the value in (1,1) as intended. An empty cell exports as one `Text` run with `""`, so it passes the check.

`patch` replaces the whole section, so only `Text` values change here — adding or removing a paragraph fails with `structural change detected`.

## Recipe C — add paragraphs under a heading (rebuild, only on explicit request)

Only when the user explicitly asks for new paragraphs in this file and accepts the loss below, a new output file, and checking it in 한컴 (SKILL.md rule 5). Never as a way around a `set-cell`/`insert-para`/`stamp` refusal.

```bash
hwpforge to-json template.hwpx -o full.json
#   insert paragraph objects into document.sections[N].paragraphs after the heading,
#   copying para_shape_id / char_shape_id from a neighbouring body paragraph (never invent IDs)
hwpforge from-json full.json -o 제안서_제출본.hwpx --base template.hwpx --json
hwpforge diff template.hwpx 제안서_제출본.hwpx --json
```

On a 한컴-saved template the rebuild drops `Preview/*` and `META-INF/container.rdf` (the diff lists them under `package.removed`) and every line-layout cache (`warnings: ["layout cache dropped …"]`), and elements HwpForge does not model — form controls, master pages, some advanced formatting — can disappear. Tell the user, and have them open the result in 한컴 before submitting. If the template was not saved by 한컴, `insert-para` adds paragraphs without a rebuild ([structural-edit.md](structural-edit.md)). Paragraph-insert code: [editing-workflow.md](editing-workflow.md).

## Checklist before handing it over

- [ ] Output is a new file; the template is untouched
- [ ] `warnings` read and reported
- [ ] `validate` exits 0
- [ ] `diff` shows only the intended fields/paragraphs/cells (plus the expected entries in SKILL.md's table)
- [ ] (rebuild, or anything going to a government office) opened in 한컴 and checked
