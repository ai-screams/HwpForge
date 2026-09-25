# PDF Rendering (`to-pdf`)

`to-pdf` replays the line layout stored in the document (the `hp:linesegarray` cache 한컴 writes on save). It does not break lines itself, which is why its output matches what 한컴 showed — and why it needs that cache.

## The command for "한컴에서 본 것과 같게"

```bash
hwpforge to-pdf doc.hwpx -o doc.pdf --discovery platform --json
```

The input format is detected by content, so a `.hwp` (HWP5) goes in directly. Without `-o` the PDF is written next to the input with a `.pdf` extension.

## Fonts

| Option                           | Searches                                                                                                                   | Use when                                                                                                             |
| -------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `--discovery explicit` (default) | only `--font-dir` directories                                                                                              | reproducible builds with a pinned font directory — with no `--font-dir` it finds nothing and fails `FONT_UNRESOLVED` |
| `--discovery hancom`             | `--font-dir` + the 한컴오피스 bundle (`/Applications/Hancom Office HWP.app/Contents/Resources/Hnc/Shared/TTF`, macOS only) | macOS with 한컴오피스, and you want only 한컴's own faces                                                            |
| `--discovery platform`           | `--font-dir` + 한컴 bundle + system font dirs (`~/Library/Fonts`, `/Library/Fonts`, `/System/Library/Fonts` on macOS)      | the general default                                                                                                  |
| `--font-dir DIR` (repeatable)    | the given directories                                                                                                      | Linux, CI, air-gapped hosts: point it at a directory holding the document's fonts (함초롬바탕, 함초롬돋움, …)        |

A face is matched by name; there is no silent substitution. `--degraded` renders missing bold/italic or language-axis faces as regular and skips images it cannot render; **it does not rescue an unresolved body font** (`FONT_UNRESOLVED` still fails) and it changes how the page looks. Do not add it to a request for a faithful PDF; use it only when the user accepts the difference, and tell them about the warnings it produces.

## Which documents carry a layout cache

| Document                                                                                                                 | Renders?                                                                                                                        |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| `.hwpx` saved by 한컴                                                                                                    | yes                                                                                                                             |
| `.hwp` given directly to `to-pdf`                                                                                        | yes (converted with the cache internally)                                                                                       |
| `convert-hwp5 --carry-layout-cache` output                                                                               | yes — for PDF replay and comparison only; do not hand it to 한컴 to reopen                                                      |
| `convert` (Markdown) or `from-json` output                                                                               | no — `NO_RENDERABLE_CACHE`                                                                                                      |
| after `patch`                                                                                                            | yes, but changed paragraphs keep their old line breaks; text that grew overflows (`LINE_OVERFLOW` warning)                      |
| after `fill`                                                                                                             | the filled paragraphs have no cache: skipped (`PARAGRAPH_SKIPPED`), or `NO_RENDERABLE_CACHE` when a whole section loses it      |
| after `insert-para` / `delete-para`                                                                                      | paragraphs from the edit point on are skipped (`PARAGRAPH_SKIPPED`)                                                             |
| any table without its layout cache (a table in a `convert` output, or a table one of whose cell paragraphs has no cache) | always fails: `MISSING_LAYOUT_CACHE`, even without `--partial-cache-reject` (the message says "under Reject policy" regardless) |

For a PDF of edited content, open the edited `.hwpx` in 한컴, save it, then run `to-pdf`. `--partial-cache-reject` turns "skip cacheless plain paragraphs with a warning" into a failure (`MISSING_LAYOUT_CACHE`); tables are never skipped — use it when a silently incomplete PDF would be worse than none.

## Output

Success (`--json`):

```json
{
  "status": "ok",
  "input": "t.hwpx",
  "output": "t.pdf",
  "detected_format": "hwpx",
  "size_bytes": 6050,
  "warnings": [],
  "warning_counts": { "input": 0, "convert": 0, "decode": 0, "render": 0 }
}
```

The CLI object has no page count (Python's report has `pages`). Each warning is `{stage, code, message, location?}` (`location` only when the warning has one) — for example `{"stage":"render","code":"LINE_OVERFLOW","message":"line exceeds its cached box by 225423 HWPUNIT …","location":"s0/p0/l0"}`. Report them to the user.

Failure is `PDF_RENDER_FAILED` (exit 2) with the reason in a nested `cause: {stage, code, kind?, location?}` — for example `{"stage":"render","code":"NO_RENDERABLE_CACHE","location":"s0"}`, or `{"stage":"render","code":"FONT_UNRESOLVED"}` with no location:

| `cause.code`           | Next step                                                                                                                                      |
| ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| `FONT_UNRESOLVED`      | `--discovery platform`, or `--font-dir` pointing at the named font                                                                             |
| `NO_RENDERABLE_CACHE`  | the section has no layout cache — re-save in 한컴 (or, for an HWP5 source, pass the `.hwp` directly)                                           |
| `MISSING_LAYOUT_CACHE` | a table (or the paragraph hosting it) has no cache — always fatal; or, with `--partial-cache-reject`, any paragraph lacks one. Re-save in 한컴 |

A file that is neither HWPX nor HWP5 fails `UNRECOGNIZED_FORMAT`.

## Python

```python
from hwpforge import Document
res = Document.open("doc.hwpx").to_pdf(discovery="platform")   # also font_dirs=[...], degraded=, partial_cache_reject=
open("doc.pdf", "wb").write(res.data)
print(res.report)   # {'pages': 1, 'warnings': []}
```

Python's `degraded=True` behaves like the CLI flag: an unresolved body font still raises `PDF_RENDER_FAILED`.
