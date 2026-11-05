# JSON Round-Trip Editing

Edit an existing HWPX by exporting it to JSON, changing the JSON, and writing it back. Use this (not Markdown conversion) when images, styles, tables and layout must survive. For 누름틀 use `fill`, and for grid or paragraph edits on files 한컴 did not save see [structural-edit.md](structural-edit.md). Every edit ends with the verify loop in SKILL.md ("Verify every edit").

## Which surface a 한컴-saved document accepts

A document 한컴 has saved carries package entries HwpForge's encoder does not reproduce — `Preview/PrvText.txt`, `Preview/PrvImage.png`, `META-INF/container.rdf`. Nearly every `.hwpx` a person opened and saved in 한컴 has them. The surfaces that re-encode the whole package refuse such a document **fail-closed** instead of dropping them.

| Surface                      | 한컴-saved document | CLI result                                                                                            |
| ---------------------------- | ------------------- | ----------------------------------------------------------------------------------------------------- |
| `fill`                       | works               | writes the named 누름틀, keeps every other entry byte for byte                                        |
| `patch`                      | works               | text inside existing paragraphs and cells; package preserved                                          |
| `set-cell`                   | refused             | `INPUT_ENTRIES_NOT_CARRIED` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1                                   |
| `insert-para`, `delete-para` | refused             | `INPUT_ENTRIES_NOT_CARRIED` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1                                   |
| `stamp`                      | refused             | `INPUT_ENTRIES_NOT_CARRIED` (or `INPUT_NOT_ROUNDTRIP_SAFE`), exit 1                                   |
| `from-json --base`           | lossy               | succeeds; inherits images and trusted unchanged caches, drops the entries above once anything changed |
| `restyle` (MCP, Python)      | lossy               | succeeds; re-encodes like `from-json` — see [templates.md](templates.md)                              |

The refusal codes differ by interface — Python and MCP name them differently; see [errors.md](errors.md).

**The route that works on a form 한컴 wrote:** fill named 누름틀 with `fill`, change any other text (including table cells) with `to-json --section N` → edit `content.Text` → `patch`. `from-json --base` is **never** a way around a refusal from `set-cell`, `insert-para`, `delete-para` or `stamp`. Use it only when the user explicitly asks for a structural change to that file **and** accepts that the result is a new file that loses the 한컴 package entries and the layout caches around each change, and that they must check it in 한컴.

Because `fill` and `patch` keep every other entry byte for byte, `Preview/PrvText.txt` (the preview text file managers and 한컴 show before opening) still holds the old text after the edit (measured: fill and patch both). 한컴 refreshes it on the next save; mention it if the user relies on previews.

A `.hwpx` produced by `convert-hwp5` or by HwpForge itself carries no 한컴 entries, so every surface accepts it — unless reading it loses content: text inside a field whose kind does not keep it (a bookmark span, a memo field) is dropped on reading (`CONTENT_LOST field_body`), and `set-cell`, `stamp` and `restyle` then refuse with `INPUT_NOT_ROUNDTRIP_SAFE`; `patch` still edits such a file. When unsure, run the surface on the file: a refusal is immediate and writes nothing.

## Two write-back modes

| Mode      | Command            | Can do                                                                                                                                      | Cannot do                                                                                                                                  |
| --------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Text-only | `patch`            | change text in existing paragraphs **and table cells**; preserves the package and the layout caches of paragraphs the change does not reach | add/remove paragraphs (`PATCH_FAILED … structural change detected`)                                                                        |
| Rebuild   | `from-json --base` | add/remove paragraphs, any structural change; `--base` inherits images                                                                      | keep 한컴 package entries, every layout cache, or elements HwpForge does not model (form controls, master pages, some advanced formatting) |

## Layout caches after `fill` and `patch`

An edited body paragraph or a direct cell paragraph of a top-level body table keeps its original line-layout cache when the whole paragraph is proven to fit on one line before and after editing. This covers supported Hangul, digits, selected punctuation and spaces with one character style, matching size and spacing across script groups, no character effects (such as bold, italic or shadow), kerning, emphasis marks or font-space settings. Editable 누름틀 and surrounding text count toward the width together. Paragraphs containing fields other than editable click-here fields (including bookmarks and uneditable 누름틀) lose their own cache. If the position of such a field cannot be determined, every edited paragraph in that section loses its own cache. A 누름틀 with several text runs or spanning paragraphs is also uneditable. Content whose full visible text cannot be accounted for, overflow, tabs, forced breaks, objects, empty 누름틀 and missing source evidence still drop the cache for 한컴 to lay out again. A direct cell must explicitly use horizontal text and ordinary line wrapping (BREAK); each paragraph is checked, including later paragraphs in the cell. Nested tables, tables inside text boxes, notes, headers or footers, and table captions do not get this preservation. Proven one-line edits do not cause other caches to be dropped. Without another removal cause, the cell tail, the table host and the body tail retain their original caches. The same holds for a proven body edit followed by a table or field. The nested-edit, section and later-paragraph rules below still override a successful one-line check. After an edit in body text, a later paragraph that holds only text and 누름틀 keeps its cache byte for byte: its line breaks do not move, and 한컴 restacks only the vertical positions. Later caches are dropped too in these cases:

- After an edit that fails the one-line check, a later paragraph holds an object (table, picture, shape), any part of a field other than a 누름틀 (cross-reference, date, formula, hyperlink, ...; a field can run over several paragraphs, even one that starts in the edited paragraph) or a page number: from the first such paragraph to the end of its flow, because its width or shown text can change when pages move.
- The section holds, anywhere (table cells, headers and footers included), an object not placed as a character that text wraps around (the "어울림" style and the like): every later paragraph of the edited flow (and of the flows that hold it); paragraphs inside later table cells and text boxes keep theirs, because the object stays put while the text under it moves.
- The section uses vertical text or columns of different widths: every later paragraph of the edited flow (and of the flows that hold it); paragraphs inside later table cells and text boxes keep theirs.
- A field anywhere in the section (headers and footers included) cannot be tied to its place in the document, because its start and end do not match (such as a field with no end) or its position is not known (such as a field on a master page kept in a separate file): if the unlocated field is a bookmark or an uneditable click-here field, edited body paragraphs in that section lose only their own caches, while later plain-text paragraphs keep theirs. Other unlocated fields, or field errors and warnings such as mismatched starts and ends, still drop the caches of later paragraphs in the edited flow and in the flows that hold it. Other paragraphs of the section keep their caches.
- A direct cell edit fails the one-line check, or the edit is inside a nested table, text box, footnote or endnote: the later paragraphs of that flow, and the paragraph holding the table, box or note with the ones after it.
- The edit is inside a header or footer: only the later paragraphs of that header or footer; the body keeps its caches.
- `patch` has to rewrite the style definitions and so rebuilds the file: the rules above do not apply. The rebuild's own rules decide, and they can drop more caches, up to the whole section or document (for example with page numbers or floating objects); see [4b. Write back with a rebuild](#4b-write-back-with-a-rebuild).

Each dropped cache shows as a `raw` `$.layout_cache` entry in `diff`. Keeping a cache does not check it: a cache that was already stale in the input stays stale.

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
hwpforge to-json document.hwpx --section 0 --no-styles -o section0.json   # smaller, for patch only: from-json refuses it
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

Re-export with the same hwpforge version you patch with. `style_ref` and `heading_level` are optional per paragraph — copy them only if the source paragraph has them.

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

`warnings` is always present in `from-json` output. Each cache a rebuild loses is listed as `"layout cache dropped at section[0].para[0] …"`. Without `--base` no cache is written. With `--base` the trusted unchanged caches are kept and the rest are dropped; JSON identical to the base returns its bytes unchanged with `[]`. A document missing caches may fail `to-pdf` until 한컴 re-saves it. If you changed a table's shape (rows, columns, merges), delete the `addr` fields of that table's cells first — a stale `addr` is refused with `GRID_ADDR_INVALID` (a missing `addr` is not checked).

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
if "style_ref" in ref:        # optional — copy only if present
    new_para["style_ref"] = ref["style_ref"]
paras.append(new_para)
```

## Large documents

Instead of exporting everything, use `outline` + `read --section N --paras A..B` to read and `to-json --section N` to edit one section at a time. MCP `hwpforge_to_json` returns JSON inline only while the response stays under 1 MB (in practice about 750–800 KB of document JSON); beyond that it fails `OUTPUT_TOO_LARGE` — pass `output_path`. Input limits per interface (file 100 MB, stdin/inline 50 MB, Python `from_bytes` none): [errors.md](errors.md).
