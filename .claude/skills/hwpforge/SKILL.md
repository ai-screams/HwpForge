---
name: hwpforge
description: "Fill, read, edit, verify, convert and render Korean 한글/Hancom documents (.hwpx; legacy .hwp read-only) with HwpForge. Use when the user wants to fill a form or government template (누름틀, 표 칸, 빈칸, 서식 채우기) including files saved in 한컴오피스; read text, tables or fields out of one (표를 CSV로); edit paragraphs or table cells; check that an edit changed only what was intended (diff, 검증); render to PDF (PDF로, 한컴에서 본 것과 같게); create an HWPX from Markdown (보고서·제안서·공문); convert .hwp to .hwpx or .hwpx to Markdown; or do any of this from Python (Python에서, pip install hwpforge) or through hwpforge_* MCP tools. Not for .docx, editing PDFs, or Korean spell-checking."
license: MIT OR Apache-2.0
compatibility: "Needs one of, version 0.16.6 or later: the hwpforge CLI, the HwpForge MCP server (@hwpforge/mcp), or the hwpforge Python package (CPython 3.9+). Works on local files; network access only to install."
metadata:
  author: ai-screams
  version: "0.3.0"
allowed-tools: "Bash(hwpforge *)"
---

# HwpForge

Reads and writes Korean HWPX (KS X 6101); reads legacy HWP5 `.hwp`. Writes `.hwpx` only.

## Safety rules

1. **Never write over the input.** Edit into a new file (`-o doc.edited.hwpx`); the input stays as the `diff` base and the undo. Replace the original only after a clean diff, and only if the user asked for an in-place edit.
2. **Read `warnings` before reporting success.** Exit 0 is not "nothing lost". A `warnings` key that is absent or `[]` means none; otherwise report every entry.
3. **Verify every edit with `diff`** against the table below.
4. **A fail-closed refusal is an answer.** Follow its `hint` (usually `fill`, or `to-json --section N` → `patch`). Never use `from-json` to get around a refusal.
5. **`from-json --base` only on explicit request.** Use it only when the user asks for a structural change to that file and accepts that the result is a new file, loses 한컴 package entries and layout caches, and must be checked in 한컴.
6. **No raw JSON to the user.** Summarize.

## Which interface

| Situation                        | Use                                                                                                                                      |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `hwpforge` on PATH               | CLI — all commands ([commands.md](references/commands.md))                                                                               |
| `hwpforge_*` MCP tools connected | MCP — 19 tools; no PDF, HWP5 or schema tool                                                                                              |
| Writing Python                   | `pip install hwpforge` / `uv add hwpforge`; no pip → unzip the wheel ([python.md](references/python.md))                                 |
| Nothing installed                | `cargo install --git https://github.com/ai-screams/HwpForge hwpforge-bindings-cli`, or `claude mcp add hwpforge -- npx -y @hwpforge/mcp` |

Python, for example a table to CSV (full example in [python.md](references/python.md)):

```python
from hwpforge import Document
t = Document.open("form.hwpx").read(table=0)["table"]   # ordinals: doc.outline()["outline"]["tables"]
# t["rows"], t["cols"], t["cells"] = [{row, col, row_span, col_span, text}]; a merged cell appears once, at its anchor
```

## Intent → command

| Intent                                | Command                                                                             |
| ------------------------------------- | ----------------------------------------------------------------------------------- |
| Values into a form 한컴 saved         | 누름틀: `fields` → `fill`. Other text/cells: `to-json --section N` → edit → `patch` |
| Cells in a file 한컴 did not save     | `read --table N` → `set-cell`                                                       |
| New document                          | `convert doc.md -o doc.hwpx --preset default`                                       |
| Read content                          | `outline` → `read --section N --paras A..B` / `--table N` / `--field NAME`          |
| PDF like 한컴 shows it                | `to-pdf doc.hwpx -o doc.pdf --discovery platform`                                   |
| Legacy `.hwp`                         | `convert-hwp5 old.hwp -o new.hwpx`; `to-pdf old.hwp` takes it directly              |
| Did my edit change only what I meant? | `diff before.hwpx after.hwpx --json`                                                |

## Decision tree

```text
.hwp input  → convert-hwp5 first (outline/read/edits refuse it: DECODE_FAILED); to-pdf takes it directly
Read        → outline → read; to-md for people; to-json only for machine editing
Edit        → locate first (fields for 누름틀, else outline), then the narrowest surface:
  named 누름틀                       fields → fill                        works on 한컴-saved
  existing text or cell text         to-json --section N → edit → patch   works on 한컴-saved
  cell by grid address or label      set-cell                             refuses 한컴-saved  (structural-edit.md)
  placeholders → 누름틀, once        stamp-plan → spec map → stamp        refuses 한컴-saved  (stamp.md)
  add/remove top-level paragraph     insert-para / delete-para            refuses 한컴-saved  (structural-edit.md)
  larger structural change           to-json → from-json --base           rule 5 only        (editing-workflow.md)
New doc     → convert (markdown-guide.md)
PDF         → to-pdf (pdf.md)
```

**한컴-saved files** carry `Preview/*` and `META-INF/container.rdf`. Nearly every file a person saved in 한컴 does. `fill` and `patch` rewrite only the section XML and keep every untouched package entry byte for byte. `set-cell`, `insert-para`, `delete-para` and `stamp` refuse. Surface table: [editing-workflow.md](references/editing-workflow.md).

## Verify every edit

```bash
hwpforge fill form.hwpx --set 성명=홍길동 -o form.filled.hwpx --json > r.json  # 1. new file; form.hwpx stays the base
#   2. r.json: `warnings` absent or [] = none; otherwise tell the user
hwpforge validate form.filled.hwpx --json                                    # 3. exit 0
hwpforge diff form.hwpx form.filled.hwpx --json                              # 4. compare with the table
command mv form.filled.hwpx form.hwpx                                        # 5. only if asked for in-place AND 4 is clean
```

Stdout nests the report under `.diff`. The `diff -o report.json` file and Python's `doc.diff(other)` put `identical`/`semantic`/`package` at the top level.

| Edit                          | `semantic` should show                                                                                                                              | `package.changed`                                              |
| ----------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `fill`                        | `field_values` `value_changed`; `raw` `$.layout_cache` on each filled paragraph that had a cache                                                    | section XML                                                    |
| `patch`                       | `paragraphs`/`cells` you changed                                                                                                                    | section XML                                                    |
| `set-cell`                    | `cells` `{table,row,col,before,after}`                                                                                                              | `Contents/header.xml` + section XML                            |
| `stamp`                       | `field_values` `added`; marker text removed; cell stamps as `raw` with `detail` ending `.content.Control`                                           | `Contents/header.xml` + section XML                            |
| `insert-para` / `delete-para` | `structure` count change and `added`/`removed` at the edit point; if the input had layout caches, later paragraphs also show as `changed` (shifted) | section XML                                                    |
| `from-json --base`            | your changes; `raw` `$.layout_cache` on paragraphs that had a cache                                                                                 | 한컴 base: also `package.removed` `Preview/*`, `container.rdf` |

`fill` drops the line-layout cache of the paragraphs it filled so 한컴 re-flows them, so that `raw` entry is normal. The report's `note` ("layout caches … not itemized") is about the package channel only. `raw` keeps at most 100 entries; if `raw_dropped` > 0 some were not listed, so check the rest another way. **Stop** on an entry you did not intend, a `raw` on a paragraph you did not touch or with an unlisted `detail`, or `package.added`/`removed` outside the `from-json` row.

## PDF (details: [pdf.md](references/pdf.md))

- **Fonts:** use `--discovery platform` (your `--font-dir`s, the 한컴오피스 bundle, then system fonts). The default `explicit` searches only `--font-dir` and fails `FONT_UNRESOLVED` without one. Use `--font-dir DIR` on Linux, CI or air-gapped hosts.
- **Not `--degraded` for fidelity.** It renders missing bold/italic or script faces as regular and skips bad images. It does not fix an unresolved body font.
- **Needs the layout cache 한컴 saved.** A 한컴-saved `.hwpx`, a `.hwp`, or `convert-hwp5 --carry-layout-cache` output have one. `convert` and `from-json` output do not (`NO_RENDERABLE_CACHE`). After `patch`, old line breaks stay (`LINE_OVERFLOW`). After `fill`, `insert-para` or `delete-para`, plain paragraphs without cache are skipped (`PARAGRAPH_SKIPPED`), but a table without cache fails the render (`MISSING_LAYOUT_CACHE`). For edited content, re-save in 한컴 first.

## Errors (main codes: [errors.md](references/errors.md))

With `--json` a failure is one object on stderr: `{"status":"error","code","message","hint"?}`. `to-pdf` adds `cause: {stage, code, kind?, location?}`. A malformed command line prints usage text and exits 2. Branch on `code`, then follow `hint`.

| Code                                                 | Next step                                                                                             |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `INPUT_ENTRIES_NOT_CARRIED`, `UNCARRIED_ZIP_ENTRIES` | 한컴-saved input → `fill`, or `to-json --section N` → `patch`                                         |
| `FIELD_NOT_FOUND` / `EMPTY_FIELD_VALUE`              | re-run `fields` / clearing is not supported                                                           |
| `PATCH_FAILED`                                       | "structural change detected" (paragraph count changed) or "missing preservation metadata" (re-export) |
| `PDF_RENDER_FAILED`                                  | `cause.code`: `FONT_UNRESOLVED` → `--discovery platform`; `NO_RENDERABLE_CACHE` → re-save in 한컴     |

Exit codes: 0 ok · 1 refused input or missing file · 2 codec/schema failure or bad command line · 3 `validate` only. Warning shapes per command, interface-specific codes and size limits are in errors.md.

## References

| File                                                                                                                                                                    | For                                                                 |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- |
| [commands.md](references/commands.md)                                                                                                                                   | Every CLI command in its key form                                   |
| [editing-workflow.md](references/editing-workflow.md)                                                                                                                   | JSON round-trip, the 한컴-saved surface table                       |
| [template-fill.md](references/template-fill.md)                                                                                                                         | Filling a Korean template (국가과제 제안서 etc.)                    |
| [structural-edit.md](references/structural-edit.md) · [stamp.md](references/stamp.md)                                                                                   | `set-cell` / `insert-para` / `delete-para` · placeholders to 누름틀 |
| [pdf.md](references/pdf.md) · [python.md](references/python.md)                                                                                                         | PDF rendering · the Python package                                  |
| [errors.md](references/errors.md)                                                                                                                                       | Main error codes, warnings shapes, exit codes, size limits          |
| [markdown-guide.md](references/markdown-guide.md) · [templates.md](references/templates.md)                                                                             | Markdown for `convert` · presets and `restyle`                      |
| [scenario-proposal.md](references/scenario-proposal.md) · [scenario-report.md](references/scenario-report.md) · [scenario-official.md](references/scenario-official.md) | 제안서 · 보고서 · 공문                                              |
