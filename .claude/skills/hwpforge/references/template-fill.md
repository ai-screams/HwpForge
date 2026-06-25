# Filling a Korean Template (e.g. 국가과제 제안서)

A template is an existing `.hwpx` (or `.hwp`) with predefined sections, headings, tables, and
placeholder text (e.g. `(작성)`, `(여기에 … 작성하시오)`, `○○○`). The goal is to put the
user's content into it **without breaking the template's formatting** — critical for government
submissions.

## Decision: which mode?

| The template asks you to…                          | Mode          | Command            |
| -------------------------------------------------- | ------------- | ------------------ |
| Replace placeholder text (body **or table cells**) | **Text-only** | `patch`            |
| Add extra paragraphs the template did not have     | **Rebuild**   | `from-json --base` |

**Prefer `patch`.** It preserves the template byte-for-byte and only swaps text. Use the rebuild
path only when you genuinely need new paragraphs, and verify the result (see Fidelity).

## If the template is a legacy `.hwp`

There is no `.hwp` writer. Convert first, then fill the `.hwpx`:

```bash
hwpforge convert-hwp5 template.hwp -o template.hwpx
```

## Recipe A — fill placeholders (text-only, recommended)

```bash
# 1. Learn the structure
hwpforge inspect template.hwpx --json        # how many sections, where the tables are

# 2. Export the target section
hwpforge to-json template.hwpx --section 0 -o sec.json

# 3. Replace placeholder text in sec.json (keep all style IDs untouched)
#    - body placeholder:   runs[].content.Text
#    - table cell:         content.Table.rows[].cells[].paragraphs[].runs[].content.Text

# 4. Write back (text-only — preserves the whole template)
hwpforge patch template.hwpx --section 0 sec.json -o 제안서_제출본.hwpx

# 5. Verify
hwpforge inspect 제안서_제출본.hwpx
hwpforge to-md   제안서_제출본.hwpx -o check.md     # eyeball the filled content
```

Body placeholders are usually unique text, so a simple text→value map works for them:

```python
import json
d = json.load(open("sec.json"))

BODY = {
    "(연구개발 목표를 작성하시오)": "본 과제는 …를 목표로 한다.",
    "(기대효과를 작성하시오)": "행정 문서 작성 시간을 50% 단축한다.",
}

def cell_text(cell):  # first run's text of a cell, for matching the row label
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

for p in d["section"]["paragraphs"]:
    for r in p.get("runs", []):
        c = r.get("content", {})
        if c.get("Text") in BODY:
            c["Text"] = BODY[c["Text"]]
```

**Tables: fill POSITIONALLY, never by a text map.** Budget tables repeat the same placeholder
(`(작성)`) in many cells, so a `{"(작성)": value}` map would write the SAME value into every
cell. Instead, find each row by its label cell, then fill that row's columns by index:

```python
# 연구비 표:  비목 | 1년차 | 2년차   →  cells[0]=label, cells[1]=1년차, cells[2]=2년차
BUDGET = {  # row label → [1년차, 2년차]
    "인건비": ["120,000", "130,000"],
    "재료비": ["30,000", "20,000"],
}

for p in d["section"]["paragraphs"]:
    for r in p.get("runs", []):
        tbl = r.get("content", {}).get("Table")
        if not tbl:
            continue
        for row in tbl["rows"]:
            cells = row["cells"]
            label = cell_text(cells[0])
            if label in BUDGET:
                for col, value in enumerate(BUDGET[label], start=1):
                    if col < len(cells):
                        set_cell(cells[col], value)

json.dump(d, open("sec.json", "w"), ensure_ascii=False)
```

> `patch` replaces the entire section, so `sec.json` must keep every existing paragraph — only
> the placeholder **Text** values change. Do not add or remove paragraphs here (that would fail
> with `structural change detected`).

## Recipe B — add new paragraphs under a heading (rebuild)

When the template has a heading but you must add several body paragraphs:

```bash
hwpforge to-json template.hwpx -o full.json        # FULL document
#   in full.json, find the heading's section, then insert paragraph objects into
#   document.sections[N].paragraphs right after the heading.
#   Copy para_shape_id + char_shape_id from a neighboring body paragraph (never invent IDs);
#   copy style_id / heading_level only if the neighbor has them.
hwpforge from-json full.json -o 제안서_제출본.hwpx --base template.hwpx
hwpforge inspect 제안서_제출본.hwpx                  # paragraph count increased
```

See [editing-workflow.md](editing-workflow.md) for the paragraph-insert code pattern.

## Fidelity (read before submitting a government document)

- `patch` (Recipe A) preserves the original template exactly — **the safe default**.
- `from-json --base` (Recipe B) **rebuilds** the document. Simple tables and paragraphs are
  preserved, but elements HwpForge does not yet fully model — **form controls, master pages,
  some advanced formatting** — can be dropped silently.
- **Always open a rebuilt document in 한컴 and verify it visually before submission.** If a
  required form element disappears, fall back to filling placeholders with `patch` only, or hand
  the rebuild gap to a human.

## Verify checklist

- [ ] `inspect` shows the expected section / paragraph / table counts
- [ ] `to-md` output shows your content in the right places
- [ ] (government) opened in 한컴, formatting and form fields intact
