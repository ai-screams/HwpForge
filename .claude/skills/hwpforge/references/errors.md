# Main Error Codes, Warnings, Exit Codes, Size Limits

## Error envelope

With `--json`, a failure prints one object on **stderr** and nothing on stdout:

```json
{
  "status": "error",
  "code": "FIELD_NOT_FOUND",
  "message": "field 'nope' not found; available: [user_email]",
  "hint": "사용 가능한 필드: [user_email] — `hwpforge fields <file>` 로 확인"
}
```

- `hint` appears only when there is something actionable; read `code` first, then follow `hint`.
- `to-pdf` adds a nested `cause: {stage, code, kind?, location?}` (`kind` and `location` only when the failure carries them) — branch on `cause.code` ([pdf.md](pdf.md)).
- Without `--json`: `Error [CODE]: message` on stderr, then `Hint: …` when present.
- A malformed command line (missing argument, unknown flag) is reported by the argument parser as plain usage text, not JSON, with exit 2.

## Exit codes

Branch on `code`, not on the exit code — which of 1 and 2 a failure uses is fixed per (command, code).

| Exit | Meaning                                                                                                                                  |
| ---- | ---------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | success (still read `warnings`)                                                                                                          |
| 1    | as a rule a refused input: missing file, unknown field or preset, fail-closed edit refusals                                              |
| 2    | as a rule a codec/schema failure: undecodable input, bad JSON, `PATCH_FAILED`, `PDF_RENDER_FAILED`, `MD_DECODE_FAILED`, bad command line |
| 3    | `validate` only — decodes but fails validation; stdout carries `{"status":"ok","valid":false,…,"errors":[…]}`                            |

## Main codes and next steps (CLI)

The codes an agent meets most often. Not exhaustive — any other code comes with a `message` and usually a `hint`; follow those.

| Code                                                                                                                             | Exit | Cause → next step                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------- | ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `FILE_READ_FAILED`                                                                                                               | 1    | path missing or unreadable                                                                                                                                                                                                                      |
| `INPUT_TOO_LARGE`                                                                                                                | 1    | input over the size limit (see Size limits below)                                                                                                                                                                                               |
| `DECODE_FAILED`                                                                                                                  | 2    | not HWPX; a `.hwp` given to `outline`/`read`/`inspect`/edits → `convert-hwp5` first                                                                                                                                                             |
| `HWP5_DECODE_FAILED`                                                                                                             | 2    | `convert-hwp5` could not read the `.hwp`                                                                                                                                                                                                        |
| `UNKNOWN_PRESET`                                                                                                                 | 1    | preset name outside `default`/`modern`/`classic`/`latest`                                                                                                                                                                                       |
| `MD_DECODE_FAILED`                                                                                                               | 2    | raw HTML other than `<!-- hwpforge:section -->`, a definition list, or a list/block inside a footnote definition                                                                                                                                |
| `FIELD_NOT_FOUND`                                                                                                                | 1    | re-run `fields` for the exact names                                                                                                                                                                                                             |
| `FIELD_NOT_FILLABLE`                                                                                                             | 1    | field ambiguous or with an empty body — re-save in 한컴, then retry                                                                                                                                                                             |
| `EMPTY_FIELD_VALUE`                                                                                                              | 1    | `fill` cannot clear a field — do that in 한컴                                                                                                                                                                                                   |
| `INPUT_ENTRIES_NOT_CARRIED`                                                                                                      | 1    | `set-cell`/`stamp` on a 한컴-saved file → `fill`, or `to-json --section N` → `patch`                                                                                                                                                            |
| `UNCARRIED_ZIP_ENTRIES`                                                                                                          | 1    | `insert-para`/`delete-para` on a 한컴-saved file → same route                                                                                                                                                                                   |
| `INPUT_NOT_ROUNDTRIP_SAFE`                                                                                                       | 1    | input cannot be re-encoded losslessly → same route                                                                                                                                                                                              |
| `PATCH_FAILED`                                                                                                                   | 2    | "structural change detected" → you added/removed paragraphs; restore the count (a structural change is `insert-para`, or `from-json --base` only under SKILL.md rule 5); "missing preservation metadata" → re-export with `to-json --section N` |
| `SECTION_INDEX_MISMATCH`                                                                                                         | 2    | `section_index` in the JSON differs from `--section`                                                                                                                                                                                            |
| `JSON_PARSE_FAILED`                                                                                                              | 2    | the edited JSON is not valid JSON                                                                                                                                                                                                               |
| `GRID_ADDR_INVALID`                                                                                                              | 2    | stale cell `addr` after changing a table's shape → delete those `addr` fields                                                                                                                                                                   |
| `CELL_NOT_FOUND` / `CELL_LABEL_AMBIGUOUS` / `CELL_HAS_NON_TEXT_CONTENT`                                                          | 1    | see [structural-edit.md](structural-edit.md)                                                                                                                                                                                                    |
| `REFERENCE_STRANDED` / `HARD_BREAK_LOSS` / `SECTION_PROPERTIES_PARAGRAPH` / `EMPTY_SECTION` / `INSERT_BEFORE_SECTION_PROPERTIES` | 1    | paragraph edit refused — see [structural-edit.md](structural-edit.md)                                                                                                                                                                           |
| `STAMP_CANDIDATE_UNCOVERED` / `STAMP_SOURCE_HASH_MISMATCH` / `STAMP_CELL_NOT_ANCHOR` / `INVALID_STAMP_MAP`                       | 1    | see [stamp.md](stamp.md)                                                                                                                                                                                                                        |
| `READ_TABLE_OUT_OF_RANGE`                                                                                                        | 1    | table ordinal too large — check `outline`                                                                                                                                                                                                       |
| `PDF_RENDER_FAILED`                                                                                                              | 2    | branch on `cause.code`, not on `hint`: every cause and its next step is in [pdf.md](pdf.md) (the measured `INVALID_CACHE` and `UNSUPPORTED_CONTENT` kinds are not fixed by re-saving in 한컴)                                                   |
| `UNRECOGNIZED_FORMAT`                                                                                                            | 2    | `to-pdf` input is neither HWPX nor HWP5                                                                                                                                                                                                         |

## Refusal codes differ by interface

The operations are shared, but each interface keeps its own names. For a 한컴-saved input:

| Operation                                     | CLI                         | Python                      | MCP                                                        |
| --------------------------------------------- | --------------------------- | --------------------------- | ---------------------------------------------------------- |
| `set-cell` / `set_cell` / `hwpforge_set_cell` | `INPUT_ENTRIES_NOT_CARRIED` | `INPUT_ENTRIES_NOT_CARRIED` | `INPUT_ENTRIES_NOT_CARRIED`                                |
| `stamp`                                       | `INPUT_ENTRIES_NOT_CARRIED` | `INPUT_ENTRIES_NOT_CARRIED` | `INPUT_ENTRIES_NOT_CARRIED`                                |
| `insert-para` / `delete-para`                 | `UNCARRIED_ZIP_ENTRIES`     | `INPUT_ENTRIES_NOT_CARRIED` | `STRUCTURAL_EDIT_FAILED` (the hint names the 한컴 entries) |

`INPUT_NOT_ROUNDTRIP_SAFE` is the same everywhere. An unknown preset is `UNKNOWN_PRESET` in the CLI and `PRESET_NOT_FOUND` in Python.

## Warnings

Exit 0 is not "nothing was lost". A `warnings` key that is absent or `[]` means none; otherwise tell the user about every entry. The key and entry shape differ by command (measured on 0.16.6 with inputs that do and do not warn):

| Command (CLI `--json`)                                                                     | When empty                                                            | Entry shape                                                                                                  |
| ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| `fill`, `patch`, `set-cell`, `stamp`, `stamp-plan`, `fields`, `read`, `outline`, `inspect` | key omitted                                                           | `{"code","message"}` object                                                                                  |
| `validate`                                                                                 | `[]` present                                                          | `{"code","message"}` object                                                                                  |
| `insert-para`, `delete-para`                                                               | `[]` present                                                          | string `"CODE: message"`                                                                                     |
| `convert`                                                                                  | key omitted                                                           | string                                                                                                       |
| `from-json`                                                                                | `[]` present                                                          | string                                                                                                       |
| `to-pdf`                                                                                   | `[]` present, plus `warning_counts: {input, convert, decode, render}` | `{stage, code, message, location?}`                                                                          |
| `convert-hwp5`                                                                             | `warnings: 0`                                                         | `warnings` is a **count**; the list is `warning_details` (strings)                                           |
| `to-md`                                                                                    | —                                                                     | not in the success object: each warning is a separate `{"status":"warning","code","message"}` line on stderr |
| Python (every report but `templates()`/`schema()`)                                         | `[]` present                                                          | `{"code","message"}` object                                                                                  |

Without `--json`, warnings go to stderr as `[command] …` lines (`Warning: …` for `to-md`). Common ones:

| Warning                                                                 | From                                                                | Meaning                                                                                                                 |
| ----------------------------------------------------------------------- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `image "…" not embedded (…) — image dropped`                            | `convert`                                                           | file missing, outside the Markdown's directory, remote URL, unsupported format, or stdin input without a base directory |
| `LAYOUT_CACHE_DROPPED` / `layout cache dropped at section[i].para[j] …` | `from-json`; any command decoding a document whose cache is invalid | that paragraph has no usable line-layout cache; `to-pdf` will skip it until 한컴 re-saves                               |
| `LINE_OVERFLOW`                                                         | `to-pdf`                                                            | text no longer fits the cached line (after `patch`)                                                                     |
| `PARAGRAPH_SKIPPED`                                                     | `to-pdf`                                                            | a paragraph without layout cache was left out of the PDF                                                                |

## Size limits

| Input                                                                              | Limit  | Over the limit                                                                                                                   |
| ---------------------------------------------------------------------------------- | ------ | -------------------------------------------------------------------------------------------------------------------------------- |
| Main document path (CLI positional input, MCP `file_path`, Python `Document.open`) | 100 MB | `INPUT_TOO_LARGE`, exit 1 in the CLI (a pipe or FIFO whose size is unknown can instead fail as that command's read/decode error) |
| CLI auxiliary input: `stamp --map`, `set-cell --map`                               | 100 MB | `FILE_READ_FAILED` "Cannot read '…': input exceeds 100 MB limit", exit 1                                                         |
| CLI auxiliary input: `from-json --base`                                            | 100 MB | `INPUT_TOO_LARGE` for a regular file (measured); `FILE_READ_FAILED` when the size is unknown in advance                          |
| CLI stdin (`convert -`)                                                            | 50 MB  | `INPUT_TOO_LARGE` "Stdin input exceeds 50 MB limit"                                                                              |
| MCP inline Markdown (`hwpforge_convert` with `is_file: false`)                     | 50 MB  | `INPUT_TOO_LARGE` — write to a file and pass `is_file: true`                                                                     |
| MCP inline JSON (`hwpforge_from_json` `structure`)                                 | 50 MB  | `INPUT_TOO_LARGE`                                                                                                                |
| MCP inline JSON response (`hwpforge_to_json`, `hwpforge_outline`, `hwpforge_diff`) | 1 MB   | `OUTPUT_TOO_LARGE` — pass `output_path`                                                                                          |
| Python `Document.from_bytes(data)`                                                 | none   | the caller already holds the bytes                                                                                               |

For a large document, read with `outline` + `read --section N --paras A..B` and edit one section at a time with `to-json --section N`.
