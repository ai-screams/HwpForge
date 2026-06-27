# JSON Round-Trip Editing Workflow

Edit an existing HWPX by exporting it to JSON, changing the JSON, and writing it back.
Use this (NOT Markdown conversion) when you must preserve images, styles, tables, and layout.

## Two edit modes — choose correctly

| Mode          | Command            | Use for                                                                                     | Limit                                                                                 |
| ------------- | ------------------ | ------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| **Text-only** | `patch`            | changing text inside existing paragraphs and **table cells** (fill placeholders, fix typos) | **cannot** add/remove paragraphs — returns `PATCH_FAILED: structural change detected` |
| **Rebuild**   | `from-json --base` | adding/removing paragraphs, structural edits                                                | rebuilds from the model; complex 한컴 form elements may be lost (verify in 한컴)      |

If you only change existing text → `patch` (safest, preserves everything exactly).
If you add or remove paragraphs → `from-json --base`.

## Step-by-step

### 1. Inspect — understand structure (always first)

```bash
hwpforge inspect document.hwpx --json
```

Returns section count, per-section paragraph counts, and table/image/chart locations.

### 2. Export to JSON

```bash
hwpforge to-json document.hwpx --section 0 -o section0.json   # one section (for patch)
hwpforge to-json document.hwpx -o full.json                   # whole document (for rebuild)
hwpforge to-json document.hwpx --section 0 --no-styles -o section0.json  # smaller JSON
```

`-o/--output` is **required** — there is no stdout export.

### 3. Edit the JSON

Section export (`--section`) follows `ExportedSection`:

```jsonc
{
  "section_index": 0,
  "section": {
    "paragraphs": [
      {
        "runs": [
          { "content": { "Text": "본문 텍스트입니다." }, "char_shape_id": 7 },
        ],
        "para_shape_id": 20,
        "column_break": false,
        "page_break": false,
        // "style_id" and "heading_level" are OPTIONAL — present only on some paragraphs
      },
    ],
  },
  "styles": {},
}
```

Full export (no `--section`) follows `ExportedDocument`: `{ "document": { "sections": [ … ] }, "styles": { } }`
where each section has the same `paragraphs` shape.

**Safe to edit:**

- `…runs[].content.Text` — the visible text (text-only edits work with `patch`)
- `…runs[].char_shape_id`, `…para_shape_id` — but only to **IDs that already exist** in this document
- Table cell text: `…content.Table.rows[].cells[].paragraphs[].runs[].content.Text`

**Do not:**

- Invent new `char_shape_id` / `para_shape_id` / `style_id` values — copy from a neighboring paragraph
- Hand-edit the `styles` registry — change styles via `--preset` instead
- Change `section_index` — it must match the `--section` argument in `patch`

### 4a. Write back — text-only (`patch`)

```bash
hwpforge patch document.hwpx --section 0 section0.json -o document.hwpx
```

`patch` **replaces the entire section**, so `section0.json` must contain ALL existing paragraphs
plus your edits (read-modify-write the full section, not a delta). The first positional argument
(base HWPX) supplies image/OLE inheritance. If you added or removed paragraphs, `patch` fails with
`structural change detected` → use 4b instead.

### 4b. Write back — structural rebuild (`from-json --base`)

```bash
hwpforge from-json full.json -o document.hwpx --base document.hwpx
```

Use after adding/removing paragraphs. `--base` inherits images from the original.

### 5. Verify

```bash
hwpforge inspect document.hwpx        # paragraph/table counts as expected?
hwpforge to-md document.hwpx -o check.md   # eyeball the content
```

## Common patterns

### Replace text in a paragraph (text-only → patch)

```python
for para in data["section"]["paragraphs"]:
    for run in para.get("runs", []):
        c = run.get("content", {})
        if "Text" in c and "기존 텍스트" in c["Text"]:
            c["Text"] = c["Text"].replace("기존 텍스트", "새 텍스트")
```

### Fill table cells (text-only → patch)

Tables usually repeat the same placeholder (e.g. `(작성)`) in many cells, so matching by text
would write the same value everywhere. Fill **positionally**: find each row by its label cell,
then set its columns by index.

```python
def cell_text(cell):
    for cp in cell.get("paragraphs", []):
        for r in cp.get("runs", []):
            if "Text" in r.get("content", {}):
                return r["content"]["Text"]
    return ""

def set_cell(cell, value):
    for cp in cell.get("paragraphs", []):
        for r in cp.get("runs", []):
            if "Text" in r.get("content", {}):
                r["content"]["Text"] = value
                return

BUDGET = {"인건비": ["120,000", "130,000"], "재료비": ["30,000", "20,000"]}  # label → columns

for para in data["section"]["paragraphs"]:
    for run in para.get("runs", []):
        tbl = run.get("content", {}).get("Table")
        if not tbl:
            continue
        for row in tbl["rows"]:
            cells = row["cells"]
            cols = BUDGET.get(cell_text(cells[0]))   # cells[0] = row label
            if cols:
                for i, value in enumerate(cols, start=1):
                    if i < len(cells):
                        set_cell(cells[i], value)
```

### Add a new paragraph (structural → from-json --base)

```python
# Operate on the FULL document JSON (to-json without --section)
paras = data["document"]["sections"][0]["paragraphs"]
ref = paras[-1]  # copy a neighboring paragraph's style references
new_para = {
    "runs": [{"content": {"Text": "추가할 내용입니다."},
              "char_shape_id": ref["runs"][0]["char_shape_id"]}],
    "para_shape_id": ref["para_shape_id"],
    "column_break": False,
    "page_break": False,
}
if "style_id" in ref:        # optional — copy only if present
    new_para["style_id"] = ref["style_id"]
paras.append(new_para)
# then: hwpforge from-json full.json -o document.hwpx --base document.hwpx
```

## Schema

```bash
hwpforge schema exported-document    # full-document JSON shape
hwpforge schema exported-section     # single-section JSON shape
```

## Tips

- Inspect before, verify after.
- Use `--section N` to minimize JSON size (token efficiency) for text-only edits.
- Back up the original before writing.
- Government / 한컴-authored templates: prefer `patch` (preserves everything); if you must
  rebuild, open the result in 한컴 and check it before submitting.
