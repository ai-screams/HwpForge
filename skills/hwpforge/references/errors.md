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

In 0.18, editor no-op validation, encoding, and re-decoding failures use `INPUT_NOT_ROUNDTRIP_SAFE` (exit 1). This changes paragraph-edit and stamp no-op admission failures from exit 2 to 1. Set-cell input decoding and original package reads use `DECODE_FAILED` (exit 2, previously 1). Paragraph-edit output decoding, artifact reads, and fragment parsing use `SELF_VERIFY_FAILED` (exit 1, previously 2). Other exit policies are unchanged.

## Main codes and next steps (CLI)

The codes an agent meets most often. Not exhaustive — any other code comes with a `message` and usually a `hint`; follow those.

| Code                                                                                                                             | Exit  | Cause → next step                                                                                                                                                                                                                               |
| -------------------------------------------------------------------------------------------------------------------------------- | ----- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `FILE_NOT_FOUND`                                                                                                                 | 1 / 2 | input path does not exist; check the path                                                                                                                                                                                                       |
| `FILE_READ_FAILED`                                                                                                               | 1 / 2 | unreadable path; missing input uses `FILE_NOT_FOUND`                                                                                                                                                                                            |
| `INPUT_TOO_LARGE`                                                                                                                | 1 / 2 | input over the size limit (see Size limits below)                                                                                                                                                                                               |
| `DECODE_FAILED`                                                                                                                  | 2     | not HWPX; a `.hwp` given to `outline`/`read`/`inspect`/edits → `convert-hwp5` first                                                                                                                                                             |
| `HWP5_DECODE_FAILED`                                                                                                             | 2     | `convert-hwp5` could not decode the `.hwp`                                                                                                                                                                                                      |
| `PRESET_NOT_FOUND`                                                                                                               | 1     | preset name outside `default`/`modern`/`classic`/`latest`                                                                                                                                                                                       |
| `MD_DECODE_FAILED`                                                                                                               | 2     | raw HTML other than `<!-- hwpforge:section -->`, a definition list, or a list/block inside a footnote definition                                                                                                                                |
| `FIELD_NOT_FOUND`                                                                                                                | 1     | re-run `fields` for the exact names                                                                                                                                                                                                             |
| `FIELD_NOT_FILLABLE`                                                                                                             | 1     | field ambiguous or with an empty body — re-save in 한컴, then retry                                                                                                                                                                             |
| `EMPTY_FIELD_VALUE`                                                                                                              | 1     | `fill` cannot clear a field — do that in 한컴                                                                                                                                                                                                   |
| `INPUT_ENTRIES_NOT_CARRIED`                                                                                                      | 1     | `set-cell`/`stamp`/`insert-para`/`delete-para` on a 한컴-saved file → `fill`, or `to-json --section N` → `patch`                                                                                                                                |
| `INPUT_NOT_ROUNDTRIP_SAFE`                                                                                                       | 1     | input cannot be re-encoded losslessly → same route                                                                                                                                                                                              |
| `PATCH_FAILED`                                                                                                                   | 2     | "structural change detected" → you added/removed paragraphs; restore the count (a structural change is `insert-para`, or `from-json --base` only under SKILL.md rule 5); "missing preservation metadata" → re-export with `to-json --section N` |
| `SECTION_INDEX_MISMATCH`                                                                                                         | 2     | `section_index` in the JSON differs from `--section`                                                                                                                                                                                            |
| `JSON_PARSE_FAILED`                                                                                                              | 2     | the edited JSON is not valid JSON                                                                                                                                                                                                               |
| `GRID_ADDR_INVALID`                                                                                                              | 2     | stale cell `addr` after changing a table's shape → delete those `addr` fields                                                                                                                                                                   |
| `CELL_NOT_FOUND` / `CELL_LABEL_AMBIGUOUS` / `CELL_HAS_NON_TEXT_CONTENT`                                                          | 1     | see [structural-edit.md](structural-edit.md)                                                                                                                                                                                                    |
| `REFERENCE_STRANDED` / `HARD_BREAK_LOSS` / `SECTION_PROPERTIES_PARAGRAPH` / `EMPTY_SECTION` / `INSERT_BEFORE_SECTION_PROPERTIES` | 1     | paragraph edit refused — see [structural-edit.md](structural-edit.md)                                                                                                                                                                           |
| `STAMP_CANDIDATE_UNCOVERED` / `STAMP_SOURCE_HASH_MISMATCH` / `STAMP_CELL_NOT_ANCHOR` / `INVALID_STAMP_MAP`                       | 1     | see [stamp.md](stamp.md)                                                                                                                                                                                                                        |
| `TABLE_NOT_FOUND`                                                                                                                | 1     | table ordinal too large — check `outline`                                                                                                                                                                                                       |
| `PDF_RENDER_FAILED`                                                                                                              | 2     | branch on `cause.code`, not on `hint`: every cause and its next step is in [pdf.md](pdf.md) (the measured `INVALID_CACHE` and `UNSUPPORTED_CONTENT` kinds are not fixed by re-saving in 한컴)                                                   |
| `UNRECOGNIZED_FORMAT`                                                                                                            | 2     | `to-pdf` input is neither HWPX nor HWP5                                                                                                                                                                                                         |
| `FILE_WRITE_FAILED`                                                                                                              | 1 / 2 | output could not be written or stat failed; check output path and permissions (command exceptions below)                                                                                                                                        |

## Shared refusal codes

CLI, MCP and Python use the same canonical codes: `INPUT_ENTRIES_NOT_CARRIED` for uncarried ZIP entries, `INPUT_NOT_ROUNDTRIP_SAFE` for admission failures, `PRESET_NOT_FOUND` for unknown presets, and `ENCODE_SEMANTIC_LOSS` for semantic loss. Section and paragraph range failures remain distinct (`SECTION_OUT_OF_RANGE` / `PARAGRAPH_OUT_OF_RANGE`). Hints and CLI exit values depend on the operation.

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

| Warning                                                                 | From                                                                                                                    | Meaning                                                                                                                 |
| ----------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `image "…" not embedded (…) — image dropped`                            | `convert`                                                                                                               | file missing, outside the Markdown's directory, remote URL, unsupported format, or stdin input without a base directory |
| `LAYOUT_CACHE_DROPPED` / `layout cache dropped at section[i].para[j] …` | `from-json`; `convert-hwp5` (a line start it cannot carry over); any command decoding a document whose cache is invalid | that paragraph has no usable line-layout cache; `to-pdf` will skip it until 한컴 re-saves                               |
| `LINE_OVERFLOW`                                                         | `to-pdf`                                                                                                                | text no longer fits the cached line                                                                                     |
| `PARAGRAPH_SKIPPED`                                                     | `to-pdf`                                                                                                                | a paragraph without layout cache was left out of the PDF                                                                |

## Size limits

| Input                                                                              | Limit  | Over the limit                                                                                        |
| ---------------------------------------------------------------------------------- | ------ | ----------------------------------------------------------------------------------------------------- |
| Main document path (CLI positional input, MCP `file_path`, Python `Document.open`) | 100 MB | `INPUT_TOO_LARGE`, exit 1 in the CLI (both regular-file preflight and bounded reads, including FIFOs) |
| CLI auxiliary input: `stamp --map`, `set-cell --map`                               | 100 MB | `INPUT_TOO_LARGE`, exit 1                                                                             |
| CLI auxiliary input: `from-json --base`                                            | 100 MB | `INPUT_TOO_LARGE`, exit 1                                                                             |
| CLI stdin (`convert -`)                                                            | 50 MB  | `INPUT_TOO_LARGE`, exit 1                                                                             |
| MCP inline Markdown (`hwpforge_convert` with `is_file: false`)                     | 24 MiB | `INPUT_TOO_LARGE` — write to a file and pass `is_file: true`                                          |
| MCP inline JSON (`hwpforge_from_json` `structure`)                                 | 24 MiB | `INPUT_TOO_LARGE`                                                                                     |
| MCP inline JSON response (`hwpforge_to_json`, `hwpforge_outline`, `hwpforge_diff`) | 1 MB   | `OUTPUT_TOO_LARGE` — pass `output_path`                                                               |
| Python `Document.from_bytes(data)`                                                 | none   | the caller already holds the bytes                                                                    |

For a large document, read with `outline` + `read --section N --paras A..B` and edit one section at a time with `to-json --section N`.

Input missing/read failures exit 2 for `to-md` and `convert-hwp5`, otherwise 1. `INPUT_TOO_LARGE` always exits 1, including regular-file preflight and FIFO overflow. Output I/O exits 2 for `convert-hwp5`, `delete-para`, `insert-para`, otherwise 1.

`convert-hwp5` uses `FILE_WRITE_FAILED`/exit 2 for both write failures and stat failures after a successful write. In 0.18, FIFO overflow for `to-md`/`convert-hwp5` changes from exit 2 to 1, and post-write stat failure for `convert-hwp5` changes from exit 1 to 2.
