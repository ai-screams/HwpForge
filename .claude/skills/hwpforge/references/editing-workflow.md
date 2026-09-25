# JSON Round-Trip Editing

Edit an existing HWPX by exporting it to JSON, changing the JSON, and writing it back. Use this (not Markdown conversion) when images, styles, tables and layout must survive. For 누름틀 use `fill`, and for grid or paragraph edits on files 한컴 did not save see [structural-edit.md](structural-edit.md). Every edit ends with the verify loop in SKILL.md ("Verify every edit").

## Which surface a 한컴-saved document accepts

A document 한컴 has saved carries package entries HwpForge's encoder does not reproduce — `Preview/PrvText.txt`, `Preview/PrvImage.png`, `META-INF/container.rdf`. Nearly every `.hwpx` a person opened and saved in 한컴 has them. The surfaces that re-encode the whole package refuse such a document **fail-closed** instead of dropping them.

| Surface                      | 한컴-saved document | CLI result                                                                |
| ---------------------------- | ------------------- | ------------------------------------------------------------------------- |
| `fill`                       | works               | writes the named 누름틀, keeps every other entry byte for byte            |
| `patch`                      | works               | text inside existing paragraphs and cells; package preserved              |
| `set-cell`                   | refused             | `INPUT_ENTRIES_NOT_CARRIED` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1       |
| `insert-para`, `delete-para` | refused             | `UNCARRIED_ZIP_ENTRIES` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1           |
| `stamp`                      | refused             | `INPUT_ENTRIES_NOT_CARRIED` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1       |
| `from-json --base`           | lossy               | succeeds; inherits images, drops the entries above and every layout cache |
| `restyle` (MCP, Python)      | lossy               | succeeds; re-encodes like `from-json` — see [templates.md](templates.md)  |

The refusal codes differ by interface — Python and MCP name them differently; see [errors.md](errors.md).

**The route that works on a form 한컴 wrote:** fill named 누름틀 with `fill`, change any other text (including table cells) with `to-json --section N` → edit `content.Text` → `patch`. `from-json --base` is **never** a way around a refusal from `set-cell`, `insert-para`, `delete-para` or `stamp`. Use it only when the user explicitly asks for a structural change to that file **and** accepts that the result is a new file that loses the 한컴 package entries and layout caches, and that they must check it in 한컴.

Because `fill` and `patch` keep every other entry byte for byte, `Preview/PrvText.txt` (the preview text file managers and 한컴 show before opening) still holds the old text after the edit (measured: fill and patch both). 한컴 refreshes it on the next save; mention it if the user relies on previews.

A `.hwpx` produced by `convert-hwp5` or by HwpForge itself carries no 한컴 entries, so every surface accepts it. When unsure, run the surface on the file: a refusal is immediate and writes nothing.

## Two write-back modes

| Mode      | Command            | Can do                                                                                          | Cannot do                                                                                                                             |
| --------- | ------------------ | ----------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- |
| Text-only | `patch`            | change text in existing paragraphs **and table cells**; preserves the package and layout caches | add/remove paragraphs (`PATCH_FAILED … structural change detected`)                                                                   |
| Rebuild   | `from-json --base` | add/remove paragraphs, any structural change; `--base` inherits images                          | keep 한컴 package entries, layout caches, or elements HwpForge does not model (form controls, master pages, some advanced formatting) |

## Steps

### 1. Locate the target

```bash
hwpforge outline document.hwpx --json          # headings, tables (ordinal + at {section, para}), fields
hwpforge read document.hwpx --section 0 --paras 0..5
```

### 2. Export

```bash
hwpforge to-json document.hwpx --section 0 -o section0.json   # one section (for patch)
hwpforge to-json document.hwpx -o full.json                   # whole document (for from-json)
hwpforge to-json document.hwpx --section 0 --no-styles -o section0.json   # smaller
```

`-o` is required — there is no stdout export. A section export has the top-level keys `section_index`, `section`, `styles` (omitted with `--no-styles`) and `preservation`; paragraphs carry `layout_cache`, table cells carry `addr`. A full export is `{"document": {"sections": [...]}, "styles": {...}}`. `hwpforge schema exported-section` / `exported-document` print the JSON Schemas.

### 3. Edit the JSON

Edit the exported file in place and write it back whole — `patch` replaces the entire section, so the file must keep every existing paragraph.

| Safe to edit                                                                      | Do not                                                                              |
| --------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| `…runs[].content.Text`                                                            | remove or hand-edit `preservation` (`PATCH_FAILED … missing preservation metadata`) |
| table cell text: `…content.Table.rows[].cells[].paragraphs[].runs[].content.Text` | change `section_index` (`SECTION_INDEX_MISMATCH`)                                   |
| `char_shape_id` / `para_shape_id`, but only to IDs already in this document       | invent style IDs, or edit the `styles` registry (use `--preset` for new documents)  |
|                                                                                   | touch `layout_cache` or cell `addr` — leave them as exported                        |

Re-export with the same hwpforge version you patch with. `style_id` and `heading_level` are optional per paragraph — copy them only if the source paragraph has them.

### 4a. Write back text-only

```bash
hwpforge patch document.hwpx --section 0 section0.json -o document.edited.hwpx --json
```

The first positional argument is the base whose package is preserved.

### 4b. Write back with a rebuild

Only under SKILL.md rule 5: the user explicitly asked for this structural change and accepted a new, lossy output that they will check in 한컴. Never as a way around a refusal.

```bash
hwpforge from-json full.json -o document.edited.hwpx --base document.hwpx --json
```

`warnings` is always present in `from-json` output. If the input had layout caches, expect `["layout cache dropped at section[0].para[0] …", …]` (for a cacheless input it is `[]`): the rebuild never re-emits layout caches, so the output cannot go through `to-pdf` until 한컴 re-saves it. If you changed a table's shape (rows, columns, merges), delete the `addr` fields of that table's cells first — a stale `addr` is refused with `GRID_ADDR_INVALID` (a missing `addr` is not checked).

### 5. Verify

Run the loop in SKILL.md: `validate`, then `diff document.hwpx document.edited.hwpx --json` against the expected-delta table.

## Code patterns

Replace text in a paragraph (for `patch`):

```python
import json
d = json.load(open("section0.json"))
for para in d["section"]["paragraphs"]:
    for run in para.get("runs", []):
        c = run.get("content", {})
        if "Text" in c and "기존 텍스트" in c["Text"]:
            c["Text"] = c["Text"].replace("기존 텍스트", "새 텍스트")
json.dump(d, open("section0.json", "w"), ensure_ascii=False)
```

Fill table cells by logical grid address (for `patch` — the only cell route on a 한컴-saved file): the tested all-or-nothing script, which selects cells by `addr`, refuses multi-run or multi-paragraph cells and writes nothing unless every requested value lands exactly once, is Recipe B in [template-fill.md](template-fill.md).

Add a paragraph (full-document JSON, for `from-json --base`; on a file 한컴 did not save, `insert-para` is the byte-preserving alternative):

```python
paras = data["document"]["sections"][0]["paragraphs"]
ref = paras[-1]  # copy a neighbouring paragraph's style references
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
```

## Large documents

Instead of exporting everything, use `outline` + `read --section N --paras A..B` to read and `to-json --section N` to edit one section at a time. MCP `hwpforge_to_json` returns JSON inline only while the response stays under 1 MB (in practice about 750–800 KB of document JSON); beyond that it fails `OUTPUT_TOO_LARGE` — pass `output_path`. Input limits per interface (file 100 MB, stdin/inline 50 MB, Python `from_bytes` none): [errors.md](errors.md).
