# Grid and Paragraph Edits (`set-cell`, `insert-para`, `delete-para`)

These surfaces edit cells and top-level paragraphs without a JSON round-trip and preserve every other byte. They re-encode the package, so **a 한컴-saved document is refused** (`INPUT_ENTRIES_NOT_CARRIED` / `UNCARRIED_ZIP_ENTRIES`, exit 1) — use `fill` or `to-json --section N` → `patch` there ([editing-workflow.md](editing-workflow.md)). They accept documents HwpForge produced and `convert-hwp5` output. Write to a new file and run the verify loop in SKILL.md after each.

## `set-cell` — table cells by grid address

```bash
hwpforge read form.hwpx --table 0                                          # grid: [r,c] text, merged cells once at their anchor
hwpforge set-cell form.hwpx --table 0 --at "1,2" --text "홍길동" -o out.hwpx
hwpforge set-cell form.hwpx --table 0 --right-of "성명" --text "홍길동" -o out.hwpx
hwpforge set-cell form.hwpx --table 0 --below "비고" --text "" -o out.hwpx   # "" clears
hwpforge set-cell form.hwpx --map cells.json -o out.hwpx                   # batch, all-or-nothing
```

`cells.json` is an array of `{"table":0,"at":{"row":1,"col":2},"text":"…"}`. `--table` is the ordinal from `outline` (document order, 0-based).

| Behaviour          | Detail                                                                                                      |
| ------------------ | ----------------------------------------------------------------------------------------------------------- |
| Covered coordinate | resolves to its merge anchor; the result reports `requested`, `anchor`, `resolution: "covered_to_anchor"`   |
| Labels             | NFC + whitespace-normalized exact match; more than one match → `CELL_LABEL_AMBIGUOUS` (use `--at`)          |
| Refused cells      | a cell holding a table, image or control → `CELL_HAS_NON_TEXT_CONTENT`; outside the grid → `CELL_NOT_FOUND` |
| Diff               | `cells` entries; `package.changed` lists `Contents/header.xml` and the section XML — both normal            |

## `insert-para` — add top-level paragraphs

```bash
hwpforge insert-para doc.hwpx --section 0 --anchor 3 --text "추가 문단" -o out.hwpx
hwpforge insert-para doc.hwpx --section 0 --anchor 3 --before --text "앞에 추가" -o out.hwpx
hwpforge insert-para doc.hwpx --section 0 --anchor 3 --text "하나" --text "둘" -o out.hwpx   # contiguous block
```

- New paragraphs inherit the anchor's paragraph and character shape — no style is invented. `--text` is one line of plain text.
- Inserting before the section's first paragraph (it carries the section properties) → `INSERT_BEFORE_SECTION_PROPERTIES`.
- The inserted paragraphs and those after them have no layout cache, so `to-pdf` skips them (`PARAGRAPH_SKIPPED`) until 한컴 re-saves the file.

## `delete-para` — remove top-level paragraphs

```bash
hwpforge delete-para doc.hwpx --section 0 --index 5 -o out.hwpx
hwpforge delete-para doc.hwpx --section 0 --index 5 --index 7 -o out.hwpx   # batch
```

Fail-closed refusals (exit 1):

| Code                           | Paragraph                                                               |
| ------------------------------ | ----------------------------------------------------------------------- |
| `REFERENCE_STRANDED`           | carries a bookmark, cross-reference, footnote or other reference target |
| `HARD_BREAK_LOSS`              | carries a hard page/column break                                        |
| `SECTION_PROPERTIES_PARAGRAPH` | is the section's first paragraph                                        |
| `EMPTY_SECTION`                | the deletion would leave the section empty                              |

Deleting a paragraph that holds an index mark succeeds with an advisory in `warnings` (JSON array; stderr without `--json`).

## Reading the diff of a paragraph edit

`diff` first strips the paragraphs that are identical at the start and at the end of the section, then compares the remaining middle by position. Paragraphs compare equal only when their layout cache is equal too, so the report depends on the input (measured on `plain_inserted.hwp` converted with and without `--carry-layout-cache`, inserting after paragraph 1 and deleting paragraph 2):

| Input                                                   | `insert-para` shows                                                                                                  | `delete-para` shows                                              |
| ------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| no layout cache (HwpForge output, plain `convert-hwp5`) | one `added` at the new index                                                                                         | one `removed` at the deleted index                               |
| with layout cache                                       | the paragraphs after the edit point as `changed` (their neighbour's text in their slot), then one `added` at the end | the later paragraphs as `changed`, then one `removed` at the end |

Both carry a `structure` entry such as `section 0 paragraphs 5 → 6`. In the second case the edit strips the caches after the edit point, so the tail no longer matches. Check that the `changed` texts are the old ones shifted by the number inserted or deleted. For batch edits at several places, do not expect a particular shape; check the `structure` count and that every inserted text appears and every deleted text is gone.

Table rows cannot be added or removed with these commands; that needs `to-json` → edit → `from-json --base` (delete the edited table's cell `addr` fields first).
