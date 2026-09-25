# Python Package (`hwpforge`)

The `hwpforge` package on PyPI runs the same operations as the CLI, in-process. Use it when the user wants Python code ("Python에서", "스크립트로", "CSV로 뽑아서") — do not shell out to the CLI with `subprocess`.

## Install

```console
pip install hwpforge          # or: uv add hwpforge (project) / uv pip install hwpforge
uv run --with hwpforge python script.py   # one-off, no project
```

Wheels: CPython 3.9+ (abi3) for Linux x86_64/aarch64 (glibc 2.28+), macOS arm64/x86_64, Windows x64. No runtime dependencies.

Without pip or an index, the wheel is a zip file — download it from <https://pypi.org/project/hwpforge/#files> and unpack:

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

## Model

- `Document.open(path)` (≤ 100 MB, else `INPUT_TOO_LARGE`) or `Document.from_bytes(data)` (no size check); `save(path)`, `to_bytes()`.
- `Document` is **immutable**: every edit returns a `DocumentResult` with `.document` (the new one) and `.report`; the original stays usable as the diff base.
- Exports return `TextResult` (`.text`, `.report`) or `BytesResult` (`.data`, `.report`).
- Every report except `templates()` / `schema()` has a `warnings` key — always present; read it before saving. Entries are `{"code", "message"}` objects (e.g. `IMAGE_EMBED_SKIPPED` from `convert_md`, `LAYOUT_CACHE_DROPPED` from `from_json`), where the CLI prints plain strings.
- Failures raise `HwpForgeError`; branch on `e.code`.

## Example: a table to CSV

```python
import csv
from hwpforge import Document

doc = Document.open("form.hwpx")
print([(t["ordinal"], t["rows"], t["cols"]) for t in doc.outline()["outline"]["tables"]])

t = doc.read(table=0)["table"]
grid = [[""] * t["cols"] for _ in range(t["rows"])]
for c in t["cells"]:                      # a merged cell appears once, at its anchor
    for r in range(c["row"], c["row"] + c["row_span"]):
        for k in range(c["col"], c["col"] + c["col_span"]):
            grid[r][k] = c["text"]        # repeat the text across the merged area (or leave "" if preferred)

with open("table0.csv", "w", newline="", encoding="utf-8-sig") as f:   # utf-8-sig: Excel reads Korean correctly
    csv.writer(f).writerows(grid)
```

Run on a 3×2 form whose `비고` label spans two rows (the HwpForge repo's `tests/fixtures/tables/merged_grid_form.hwpx`) this prints `[(0, 3, 2)]` and writes `성명,` / `비고,` / `비고,`. `read(table=N)` returns `{"table": {"ordinal", "at", "rows", "cols", "cells": [{"row","col","row_span","col_span","text"}]}, "paragraphs": None, "fields": None, "warnings": [...]}`; an out-of-range ordinal raises `READ_TABLE_OUT_OF_RANGE`. `.hwp` input must go through `convert_hwp5` first.

## Example: fill a 한컴-saved form and verify

```python
from hwpforge import Document, HwpForgeError

doc = Document.open("form.hwpx")                       # 한컴-saved form
res = doc.fill({"user_email": "new@example.com"})      # doc itself is unchanged
print(res.report["warnings"])                          # []
report = doc.diff(res.document)                        # keys at top level (CLI nests them under "diff")
print(report["semantic"]["field_values"])              # [{'name': 'user_email', 'kind': 'value_changed', ...}]
print(report["semantic"]["raw"])                       # [{'path': '$.sections[0].paragraphs[0]', 'detail': '$.layout_cache'}] — normal for fill
print(report["package"])                               # {'added': [], 'removed': [], 'changed': ['Contents/section0.xml']}
res.document.save("form.filled.hwpx")                  # new file

try:
    Document.open("table.hwpx").set_cell(table=0, at="0,0", text="x")   # at is a "r,c" string; a tuple is a TypeError
except HwpForgeError as e:
    print(e.code)                                      # INPUT_ENTRIES_NOT_CARRIED on a 한컴-saved file
```

Changing cell text on a 한컴-saved file goes through `export_section` → edit → `patch`:

```python
import json
sec = json.loads(doc.export_section(section=0).text)   # keys: section_index, section, styles, preservation
# ... edit ...content.Text values in sec (keep preservation) ...
res = doc.patch(section=0, patch=json.dumps(sec, ensure_ascii=False))
print(doc.diff(res.document)["semantic"]["cells"])     # [{'table': 0, 'row': 0, 'col': 0, 'before': 'A1', 'after': '가나다'}]
```

## API

| `Document` method                                                                           | Returns          | Characteristic failures                                                      |
| ------------------------------------------------------------------------------------------- | ---------------- | ---------------------------------------------------------------------------- |
| `inspect(*, styles=False)`, `outline()`, `fields()`, `validate()`, `stamp_plan()`           | report           | `DECODE_FAILED`                                                              |
| `read(*, section, paras, table, field)` — exactly one target; `paras="0..3"` with `section` | `ReadReport`     | `READ_TARGET_REQUIRED`, `READ_TABLE_OUT_OF_RANGE`, `READ_PARA_RANGE_INVALID` |
| `diff(revised)`                                                                             | `DiffReport`     | `DECODE_FAILED`                                                              |
| `to_json(*, styles=True)`, `export_section(*, section, styles=True)`                        | `TextResult`     | `SECTION_OUT_OF_RANGE`                                                       |
| `to_md(*, mode="styled")` (`"lossy"`, `"lossless"`)                                         | `TextResult`     | `ENCODE_FAILED`                                                              |
| `to_pdf(*, font_dirs=(), discovery="explicit", degraded=False, partial_cache_reject=False)` | `BytesResult`    | `PDF_RENDER_FAILED` — pass `discovery="platform"`; see [pdf.md](pdf.md)      |
| `fill(values)`                                                                              | `DocumentResult` | `FIELD_NOT_FOUND`, `FIELD_NOT_FILLABLE`, `EMPTY_FIELD_VALUE`                 |
| `patch(*, section, patch)` — `patch` is the JSON text                                       | `DocumentResult` | `JSON_PARSE_FAILED`, `PATCH_FAILED`                                          |
| `set_cell(*, table, at, right_of, below, text, specs)`                                      | `DocumentResult` | `CELL_NOT_FOUND`, `INPUT_ENTRIES_NOT_CARRIED`                                |
| `insert_para(*, section, anchor, text, before=False)`, `delete_para(*, section, indexes)`   | `DocumentResult` | `PARAGRAPH_OUT_OF_RANGE`, `INPUT_ENTRIES_NOT_CARRIED`                        |
| `stamp(request, *, manifest=True)`                                                          | `DocumentResult` | `INPUT_ENTRIES_NOT_CARRIED`                                                  |
| `restyle(*, preset)`                                                                        | `DocumentResult` | `PRESET_NOT_FOUND` — lossy, see [templates.md](templates.md)                 |

| Module function                                                                | Returns                                                            |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `convert_md(text, *, preset="default", base_dir=None)`                         | `DocumentResult` — pass `base_dir` so relative image paths resolve |
| `from_json(text, *, base=None)` — `base` is a `Document`                       | `DocumentResult`                                                   |
| `convert_hwp5(data, *, carry_layout_cache=False)` — `data` is the `.hwp` bytes | `DocumentResult`                                                   |
| `templates()`, `schema(*, kind="document")`                                    | report                                                             |

Python names some refusals differently from the CLI (for example `INPUT_ENTRIES_NOT_CARRIED` where the CLI says `UNCARRIED_ZIP_ENTRIES`) — see [errors.md](errors.md).
