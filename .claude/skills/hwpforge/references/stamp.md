# Stamping — turning placeholders into 누름틀

A template without 누름틀 marks blanks with prose placeholders (`□`, `(   )`, `년 월 일`, `(인)`, `@`) or leaves the cell next to a label empty. `stamp` converts them into named click-here fields once, after which `fields` / `fill` work. It re-encodes the package, so **a 한컴-saved template is refused** (`INPUT_ENTRIES_NOT_CARRIED`) — there, use `to-json --section N` → `patch` ([editing-workflow.md](editing-workflow.md)).

## 1. Plan

```bash
hwpforge stamp-plan template.hwpx --json > plan.json
```

Top-level keys: `schema_version`, `source_sha256`, `candidates` (text placeholders), `cells` (label-adjacent empty cells), `skipped_tables`. A text candidate looks like:

```json
{
  "section": 0,
  "path": "paragraphs[0].runs[0].text",
  "span": { "start": 8, "end": 13 },
  "marker": "(   )",
  "pattern": "paren_blank",
  "guard": null
}
```

A cell candidate carries `table`, `section`, `at`, `labels[]` (`direction`, `at`, `raw`, `normalized`, …), `guarded`, and `suggested_name` / `suggested_hint`.

## 2. Author the spec map

Every **unguarded** candidate needs an action — `{"field":{"name":"…"}}` or `"ignore"`. Missing one → `STAMP_CANDIDATE_UNCOVERED`. Guarded candidates (inside `※`, `【작성방법】`, `(예시)` context) may be omitted.

The two spec kinds are written differently; mixing them up fails to parse (`INVALID_STAMP_MAP`):

| Spec | How to build it                                                                                                                                                                                                                                 |
| ---- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| text | **Copy the candidate object whole** and add `action`. `section`, `path`, `span` and `marker` identify it (missing one → `` missing field `section` ``); extra keys like `pattern`, `guard` are ignored                                          |
| cell | **Build a new object** with only `table`, `at`, `label` (optional), `action`. Copying the plan's cell object → `` unknown field `section` ``. A cell field needs a `hint` (`` missing field `hint` ``) because an empty cell has no marker text |

`suggested_name` is only a suggestion — two cells with the same name are refused, so rename duplicates.

Text-only map (legacy array form):

```json
[
  {
    "section": 0,
    "path": "paragraphs[0].runs[0].text",
    "span": { "start": 8, "end": 13 },
    "marker": "(   )",
    "pattern": "paren_blank",
    "guard": null,
    "action": { "field": { "name": "성명" } }
  },
  {
    "section": 0,
    "path": "…",
    "span": { "start": 0, "end": 3 },
    "marker": "□",
    "pattern": "checkbox",
    "guard": null,
    "action": "ignore"
  }
]
```

With cells, use the v2 object form (`source_sha256` copied from the plan):

```json
{
  "schema_version": 2,
  "source_sha256": "<plan value>",
  "text": [],
  "cells": [
    {
      "table": 0,
      "at": { "row": 0, "col": 1 },
      "label": { "at": { "row": 0, "col": 0 }, "text": "성명" },
      "action": { "field": { "name": "성명", "hint": "성명 입력" } }
    },
    { "table": 0, "at": { "row": 2, "col": 1 }, "action": "ignore" }
  ]
}
```

`label` (from the plan's `labels[].normalized`) re-checks that the label did not drift; omit it for a cell you name without a detected label.

## 3. Stamp and fill

```bash
hwpforge stamp template.hwpx --map specs.json -o form.hwpx --json   # also writes form.manifest.json
hwpforge fields form.hwpx --json                                    # the new fields, fillable
hwpforge fill form.hwpx --set 성명=홍길동 -o form.filled.hwpx
```

| Refusal                                                  | Meaning / next step                                                     |
| -------------------------------------------------------- | ----------------------------------------------------------------------- |
| `STAMP_CANDIDATE_UNCOVERED`                              | an unguarded candidate has no spec — classify every one                 |
| `STAMP_SOURCE_HASH_MISMATCH`                             | the document changed since the plan — re-run `stamp-plan`               |
| `STAMP_CELL_NOT_ANCHOR`                                  | a cell spec points inside a merged region; the message names the anchor |
| `INVALID_STAMP_MAP`                                      | map JSON shape is wrong (see the table above)                           |
| `INPUT_NOT_ROUNDTRIP_SAFE` / `INPUT_ENTRIES_NOT_CARRIED` | the input cannot be re-encoded losslessly (e.g. 한컴-saved)             |

In the diff, stamping shows `field_values` `added`; text stamps change the marker's paragraph or cell text, and cell stamps appear as a `raw` entry on the table's paragraph whose `detail` ends in `.content.Control`. `package.changed` lists `Contents/header.xml` and the section XML.
