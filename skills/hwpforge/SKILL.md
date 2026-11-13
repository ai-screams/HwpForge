---
name: hwpforge
description: "Fill, read, edit, verify, convert and render Korean 한글/Hancom documents (.hwpx; legacy .hwp input with .hwpx output) with HwpForge. Use when the user wants to fill a form or government template (누름틀, 표 칸, 빈칸, 서식 채우기) including files saved in 한컴오피스; read text, tables or fields out of one (표를 CSV로); edit paragraphs or table cells; check that an edit changed only what was intended (diff, 검증); render to PDF (PDF로, 한컴에서 본 것과 같게); create an HWPX from Markdown (보고서·제안서·공문); convert .hwp to .hwpx or .hwpx to Markdown; or do any of this from Python (Python에서, pip install hwpforge) or through hwpforge_* MCP tools. Not for .docx, editing PDFs, or Korean spell-checking."
license: MIT OR Apache-2.0
compatibility: "Needs one of, version 0.16.6 or later: the hwpforge CLI, the HwpForge MCP server (@hwpforge/mcp), or the hwpforge Python package (CPython 3.9+). Works on local files; network access only to install."
metadata:
  author: ai-screams
  version: "0.3.0"
allowed-tools: "Bash(hwpforge inspect *) Bash(hwpforge outline *) Bash(hwpforge read *) Bash(hwpforge fields *) Bash(hwpforge validate *) Bash(hwpforge stamp-plan *) Bash(hwpforge schema *) Bash(hwpforge templates *) Bash(hwpforge --version)"
---

# HwpForge

Reads and edits Korean HWPX (KS X 6101) and legacy HWP5 `.hwp` through supported CLI/Python operations. Writes `.hwpx` only. Automatic HWP5 input conversion requires a release newer than 0.18.0; 0.18.0 and older need explicit conversion.

## Safety rules

1. **Never write over the input.** Edit into a new file (`-o doc.edited.hwpx`); the input stays as the `diff` base and the undo. Replace the original only after a clean diff, and only if the user asked for an in-place edit.
2. **Read `warnings` and `conversion.warnings` (both roles for `diff`) before reporting success.** Exit 0 is not "nothing lost". A `warnings` key that is absent or `[]` means none; otherwise report every entry.
3. **Verify every edit with `diff`** against the table below.
4. **A fail-closed refusal is an answer.** Follow its `hint` (usually `fill`, or `to-json --section N` → `patch`). Never use `from-json` to get around a refusal.
5. **`from-json --base` only on explicit request.** Use it only when the user asks for a structural change to that file and accepts that the result is a new file, loses 한컴 package entries and the layout caches around each change, and must be checked in 한컴.
6. **No raw JSON to the user.** Summarize.

## Which interface

| Situation                        | Use                                                                                                                                      |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| `hwpforge` on PATH               | CLI — all commands ([commands.md](references/commands.md))                                                                               |
| `hwpforge_*` MCP tools connected | MCP — 19 tools; HWP5 input returns `HWP5_INPUT`; no PDF or schema tool                                                                   |
| Writing Python                   | `pip install hwpforge` / `uv add hwpforge`; no pip → unzip the wheel ([python.md](references/python.md))                                 |
| Nothing installed                | `cargo install --git https://github.com/ai-screams/HwpForge hwpforge-bindings-cli`, or `claude mcp add hwpforge -- npx -y @hwpforge/mcp` |

Python, for example a table to CSV (full example in [python.md](references/python.md)):

```python
from hwpforge import Document
t = Document.open("form.hwpx").read(table=0)["table"]   # ordinals: doc.outline()["outline"]["tables"]
# t["rows"], t["cols"], t["cells"] = [{row, col, row_span, col_span, text}]; a merged cell appears once, at its anchor
```

## Intent → command

| Intent                                     | Command                                                                                |
| ------------------------------------------ | -------------------------------------------------------------------------------------- |
| Values into a form 한컴 saved              | 누름틀: `fields` → `fill`. Other text/cells: `to-json --section N` → edit → `patch`    |
| Text-only cells by grid or label           | `read --table N` → `set-cell`                                                          |
| New document                               | `convert doc.md -o doc.hwpx --preset default`                                          |
| Read content                               | `outline` → `read --section N --paras A..B` / `--table N` / `--field NAME`             |
| PDF like 한컴 shows it                     | `to-pdf doc.hwpx -o doc.pdf --discovery platform`                                      |
| Legacy `.hwp`                              | Supported CLI/Python reads and edits convert automatically; document output is `.hwpx` |
| Did my edit change only what I meant?      | `diff before.hwpx after.hwpx --json`                                                   |
| Ready to submit? 최종본 점검 (report only) | [hwpforge-submission-check](../hwpforge-submission-check/SKILL.md) skill               |

## Decision tree

```text
.hwp input  → supported CLI/Python reads/edits convert automatically; write .hwpx
             MCP → HWP5_INPUT; convert-hwp5 first; to-pdf takes .hwp directly
Read        → outline → read; to-md for people; to-json only for machine editing
Edit        → locate first (fields for 누름틀, else outline), then the narrowest surface:
  named 누름틀                       fields → fill                        works on 한컴-saved
  existing text or cell text         to-json --section N → edit → patch   works on 한컴-saved
  cell by grid address or label      set-cell                             supported single-Text-run subset on 한컴-saved  (structural-edit.md)
  placeholders → 누름틀, once        stamp-plan → spec map → stamp        refuses 한컴-saved  (stamp.md)
  add top-level paragraph            insert-para                          refuses 한컴-saved  (structural-edit.md)
  remove plain/empty body paragraph  delete-para                          verified subset on 한컴-saved (structural-edit.md)
  larger structural change           to-json → from-json --base           rule 5 only        (editing-workflow.md)
New doc     → convert (markdown-guide.md)
PDF         → to-pdf (pdf.md)
```

HWP5 edits (`fill`, `set-cell`, `insert-para`, `delete-para`, `stamp`) refuse semantic conversion loss with `HWP5_CONVERSION_LOSS`. Review the warnings before explicitly using `--accept-conversion-loss` (Python: `accept_conversion_loss=True`); editing checks remain enforced. `patch`, `from-json --base` and Python/MCP `restyle` need explicit conversion. Policy and output protection: [errors.md](references/errors.md#hwp5-input-conversion).

**한컴-saved HWPX files** carry `Preview/*` and `META-INF/container.rdf`. Nearly every file a person saved in 한컴 does. When an edit changes text in a document with an existing Hancom preview (Preview), it removes the preview and reports PREVIEW_REMOVED. Saving again in Hancom rebuilds the preview. If the input passes the edit checks and the text is unchanged, the original file is returned. Document re-encoding, such as header normalization or style rewriting, and older whole-document JSON are outside this guarantee. `insert-para`, `stamp` and set-cell outside its preserving subset refuse. Delete can also remove verified plain or empty top-level body paragraphs from Hancom-saved HWPX. Indexes refer to positions in the input. On the source-preserving path, it removes affected line caches, sets supported affected table heights to zero for Hancom to lay out again, and removes Preview and its connections with PREVIEW_REMOVED. Saving again in Hancom rebuilds the preview. Unverifiable inputs retain their original admission error and produce no output. When every requested cell is in a direct body table with one paragraph and one Text run (including an empty run), and its source location and header can be preserved, set-cell changes only its text slot. Paragraph and table properties and unmodeled source outside the slot survive. Changed values remove existing Preview and its connections with PREVIEW_REMOVED; unchanged values return the original bytes. Input diagnostics appear once and do not imply header repair. If any cell is outside this subset or the text splice is unsupported, the whole file uses the existing round-trip and package-preservation checks and re-encoding. Address errors and verification failures return errors. Surface table: [editing-workflow.md](references/editing-workflow.md).

CONTENT_LOST reports information not represented by the input decode. Source-preserving delete succeeds only after separately proving that information survives in the output and does not affect the deletion decision; the original decode warnings remain. This applies only to some losses whose information remains in the source XML (such as per-language character values); all other CONTENT_LOST inputs retain their original refusal. Input-loss refusals for whole-document re-encoding edits remain in force.

## Verify every edit

```bash
hwpforge fill form.hwpx --set 성명=홍길동 -o form.filled.hwpx --json > r.json  # 1. new file; form.hwpx stays the base
#   2. r.json: `warnings` absent or [] = none; otherwise tell the user
hwpforge validate form.filled.hwpx --json                                    # 3. exit 0
hwpforge diff form.hwpx form.filled.hwpx --json                              # 4. compare with the table
command mv form.filled.hwpx form.hwpx                                        # 5. only if asked for in-place AND 4 is clean
```

Stdout nests the report under `.diff`. The `diff -o report.json` file and Python's `doc.diff(other)` put `identical`/`semantic`/`package` at the top level.

| Edit               | `semantic` should show                                                                                      | Normal package delta                                                           |
| ------------------ | ----------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------ |
| `fill`             | `field_values` `value_changed`; `raw` `$.layout_cache` where a cache was dropped (see below)                | section XML + Preview delta below                                              |
| `patch`            | `paragraphs`/`cells` you changed; `raw` `$.layout_cache` where a cache was dropped (see below)              | section XML + Preview delta below                                              |
| `set-cell`         | `cells` `{table,row,col,before,after}`                                                                      | section XML + Preview delta when preserved; header may change with re-encoding |
| `stamp`            | `field_values` `added`; marker text removed; cell stamps as `raw` with `detail` ending `.content.Control`   | `Contents/header.xml` + section XML                                            |
| `insert-para`      | `structure` count change and `added`; affected later caches can show shifted paragraphs as `changed`        | section XML                                                                    |
| `delete-para`      | `structure` count change and `removed`; affected later caches and supported stored table heights can change | section XML + Preview delta below                                              |
| `from-json --base` | your changes; `raw` `$.layout_cache` on paragraphs whose cache was dropped                                  | 한컴 base: also `package.removed` `Preview/*`, `container.rdf`                 |

For `fill`, `patch`, preserving `set-cell` and `delete-para` on a document with an existing Preview, a content change normally removes `Preview/PrvText.txt`, `Preview/PrvImage.png`, and the empty `Preview/` directory if present (`package.removed`). Their Preview rootfile lines are removed from `META-INF/container.xml` (`package.changed`); matching manifest items/spine references in `Contents/content.hpf` are also removed if present. Other descriptor bytes and unrelated entries, including `META-INF/container.rdf`, stay unchanged. Document re-encoding (such as header normalization) is outside this preservation guarantee; inspect that result separately before accepting it.

`fill` and `patch` keep an edited body or direct top-level table-cell paragraph’s original line-layout cache when a conservative check proves it stays on one line. Proven edits do not invalidate later caches; independent removal causes still take precedence. They drop other edited caches and the later caches affected by the layout, so 한컴 re-flows those paragraphs; those `raw` `$.layout_cache` entries are normal (which paragraphs: [editing-workflow.md](references/editing-workflow.md#layout-caches-after-fill-and-patch)). Outside its preserving subset, `set-cell` drops the edited flow tail and host chain unless the cell is proven to stay on one line without character effects (such as bold, italic or shadow), kerning, emphasis marks or font-space settings, which keeps the later caches. Character width, spacing, relative size and position settings must be present and match across all script groups; the check requires supported source fonts; review layout in 한컴. The report's `note` ("layout caches … not itemized") is about the package channel only. `raw` keeps at most 100 entries; if `raw_dropped` > 0 some were not listed, so check the rest another way. For source-preserving `delete-para`, supported affected table heights also become zero; cell caches, cell sizes and explicit table settings remain unchanged. **Stop** on an entry you did not intend, a `raw` outside the listed layout-cache and approved table-height changes, a `raw` with an unlisted `detail`, or `package.added`/`removed` outside the expected delta above (`fill`/`patch`/preserving `set-cell`/`delete-para` Preview removals or the `from-json` row). Stop on a descriptor change beyond the listed Preview connections.

## PDF (details: [pdf.md](references/pdf.md))

- **Fonts:** use `--discovery platform` (your `--font-dir`s, the 한컴오피스 bundle, then system fonts). The default `explicit` searches only `--font-dir` and fails `FONT_UNRESOLVED` without one. Use `--font-dir DIR` on Linux, CI or air-gapped hosts.
- **Not `--degraded` for fidelity.** It renders missing bold/italic faces as regular, missing characters as tofu, mixed per-script fonts with the 한글 one, and skips bad images. It does not fix an unresolved body font.
- **Needs the layout cache 한컴 saved, and content it can replay.** A 한컴-saved `.hwpx`, a `.hwp` saved with its layout, or `convert-hwp5` output (it carries the cache by default) have the cache. `convert` output and `from-json` without `--base` do not (`NO_RENDERABLE_CACHE`); `from-json --base` keeps only the trusted unchanged caches. Even with the cache, HwpForge cannot render a paragraph holding some fields (a cross-reference, date or summary; a 누름틀 or hyperlink depends on how it is stored), a footnote, equation, chart, drawn shape or anchored text box (`INVALID_CACHE`), or a table with a non-default position (`UNSUPPORTED_CONTENT`). Re-saving in 한컴 does not change that; tell the user to export the PDF from 한컴. After `fill`, `patch`, `insert-para` or `delete-para`, plain paragraphs without cache are skipped (`PARAGRAPH_SKIPPED`), but a table without cache fails the render (`MISSING_LAYOUT_CACHE`). For edited content, re-save in 한컴 first; that restores a missing cache only.

## Errors (main codes: [errors.md](references/errors.md))

With `--json` a failure is one object on stderr: `{"status":"error","code","message","hint"?}`. `to-pdf` adds `cause: {stage, code, kind?, location?}`. A malformed command line prints usage text and exits 2. Branch on `code`, then follow `hint`.

| Code                                    | Next step                                                                                                                                                                                                                                                                                         |
| --------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `INPUT_ENTRIES_NOT_CARRIED`             | 한컴-saved input → `fill`, or `to-json --section N` → `patch`                                                                                                                                                                                                                                     |
| `FIELD_NOT_FOUND` / `EMPTY_FIELD_VALUE` | re-run `fields` / clearing is not supported                                                                                                                                                                                                                                                       |
| `PATCH_FAILED`                          | "structural change detected" (paragraph count changed) or "missing preservation metadata" (re-export)                                                                                                                                                                                             |
| `PDF_RENDER_FAILED`                     | branch on `cause.code`, not `hint`: `FONT_UNRESOLVED` → `--discovery platform`; `NO_RENDERABLE_CACHE`, `MISSING_LAYOUT_CACHE` → re-save in 한컴; `INVALID_CACHE`, `UNSUPPORTED_CONTENT` → the measured kinds are not fixed by re-saving; others: report the message ([pdf.md](references/pdf.md)) |

Exit codes: 0 ok · 1 refused input or missing file (a missing or unreadable input exits 2 for `to-md`/`convert-hwp5`; `INPUT_TOO_LARGE` is always 1) · 2 codec/schema failure or bad command line · 3 `validate` only. Warning shapes per command, interface-specific codes and size limits are in errors.md.

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
