# hwpforge

Read, edit and generate Korean HWP/HWPX documents from Python.

`hwpforge` wraps the [HwpForge](https://github.com/ai-screams/HwpForge) Rust library: a
document model for the HWPX (OWPML) format, an HWP5 reader, a Markdown bridge and a PDF
renderer. The Python package is a thin layer over that library, so it performs the same
operations, with the same meaning, as the command line tool and the MCP server. What each
frontend calls them differs: method names, argument spellings and some failure codes are
Python's own public contract, and the other two keep theirs.

## What it reads and writes

Reads `.hwpx`, and `.hwp` (HWP5) through `hwpforge.convert_hwp5`. That conversion is one
way: the result is an HWPX document, and the original `.hwp` is never written back.

Writes `.hwpx` only. There is no `.hwp` output, and Hancom Office opens `.hwpx` natively.
Documents also export to Markdown, JSON and PDF. A PDF needs the document's own fonts present
on the host and named through `font_dirs` or `discovery`: by default a face that cannot be
resolved fails the render rather than being guessed at. `to_pdf(degraded=True)` relaxes that
and renders the missing face with a fallback, which changes how the page looks.

Rendering replays a layout stored in the document, so `to_pdf` needs a document that carries
one. Two do: an HWPX Hancom saved, and an HWPX converted from HWP5 with the layout carried
across — `hwpforge.convert_hwp5(data, carry_layout_cache=True).document.to_pdf(...)`. A document
this library generated from Markdown or JSON carries no layout and is refused with
`PDF_RENDER_FAILED`.

A layout carried over from HWP5 is for PDF replay and comparison only; do not treat such an
HWPX as one to reopen in Hancom.

## Install

```console
pip install hwpforge
```

With [uv](https://docs.astral.sh/uv/): `uv add hwpforge` in a project, or `uv pip install hwpforge` in an environment.

Pin with `~=` rather than `==`. A Python-only fix ships as `X.Y.Z.N`, and an exact pin never receives it.

### What is published

| Artifact                           | Platform            | Needs                 |
| ---------------------------------- | ------------------- | --------------------- |
| `cp39-abi3-manylinux_2_28_x86_64`  | Linux x86_64        | glibc 2.28 or newer   |
| `cp39-abi3-manylinux_2_28_aarch64` | Linux aarch64       | glibc 2.28 or newer   |
| `cp39-abi3-macosx_11_0_arm64`      | macOS arm64         | macOS 11 or newer     |
| `cp39-abi3-macosx_10_12_x86_64`    | macOS x86_64        | macOS 10.12 or newer  |
| `cp39-abi3-win_amd64`              | Windows x64         | —                     |
| `hwpforge-<version>.tar.gz`        | source distribution | Rust 1.92 and maturin |

One `abi3` wheel per platform covers CPython 3.9 and newer. Free-threaded builds are not
supported, because the stable ABI does not cover them.

### Hosts without an index

The wheel is a zip file and the package declares no runtime dependencies, so it can be
unpacked and imported without pip. The requirement is glibc 2.28 or newer, not a particular distribution — on any Linux x86_64 host with CPython 3.9+ (Debian 10/11/12, Ubuntu 20.04 and later; the file name below is the x86_64 wheel):

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

GitHub Releases are immutable once published, so wheels are never attached there after the
fact. Every wheel and the source distribution instead live on PyPI's
[files page](https://pypi.org/project/hwpforge/#files), with the same bytes and sha256 that
`pip` sees, so an air-gapped host can download a file straight from that page and unpack it
the same way as above. A Python-only fix, versioned `X.Y.Z.N`, is published to PyPI alone and
has no Release of its own.

## Example

```python
import hwpforge

doc = hwpforge.Document.open("proposal.hwpx")
print(doc.outline()["outline"]["title"])
doc = doc.fill({"applicant": "홍길동", "date": "2026-09-17"}).document
doc.save("proposal-filled.hwpx")
```

`Document` is immutable. Every editing method returns a new document in a result object
alongside the operation's report, and leaves the receiver unchanged, so the re-assignment
above is not optional.

## Operations

A failure raises `HwpForgeError`, whose `code` is the stable string to branch on. The codes
below are the ones each operation raises for its own characteristic failure; any operation
that has to decode the document first can also raise `DECODE_FAILED`.

### `Document` methods

| Method                                                            | Returns                            | Characteristic failures                                                                            |
| ----------------------------------------------------------------- | ---------------------------------- | -------------------------------------------------------------------------------------------------- |
| `inspect(*, styles=False)`                                        | `InspectReport`                    | `DECODE_FAILED`                                                                                    |
| `outline()`                                                       | `OutlineReport`                    | `DECODE_FAILED`                                                                                    |
| `fields()`                                                        | `FieldsReport`                     | `DECODE_FAILED`                                                                                    |
| `validate()`                                                      | `ValidateReport`                   | `DECODE_FAILED` (an invalid document reports, not raises)                                          |
| `read(*, section, paras, table, field)`                           | `ReadReport`                       | `READ_TARGET_REQUIRED`, `READ_PARAS_INVALID`, `READ_PARA_RANGE_INVALID`, `READ_TABLE_OUT_OF_RANGE` |
| `diff(revised)`                                                   | `DiffReport`                       | `DECODE_FAILED`                                                                                    |
| `stamp_plan()`                                                    | `StampPlanReport`                  | `DECODE_FAILED`                                                                                    |
| `to_json(*, styles=True)`                                         | `TextResult[ToJsonReport]`         | `DECODE_FAILED`                                                                                    |
| `export_section(*, section, styles=True)`                         | `TextResult[ExportSectionReport]`  | `SECTION_OUT_OF_RANGE`                                                                             |
| `to_md(*, mode="styled")`                                         | `TextResult[ToMdReport]`           | `ENCODE_FAILED` (`mode="lossless"` refuses to lose anything)                                       |
| `to_pdf(*, font_dirs, discovery, degraded, partial_cache_reject)` | `BytesResult[ToPdfReport]`         | `PDF_RENDER_FAILED`                                                                                |
| `fill(values)`                                                    | `DocumentResult[FillReport]`       | `FIELD_NOT_FOUND`, `FIELD_NOT_FILLABLE`, `EMPTY_FIELD_VALUE`                                       |
| `set_cell(*, table, at, right_of, below, text, specs)`            | `DocumentResult[SetCellReport]`    | `TABLE_NOT_FOUND`, `CELL_NOT_FOUND`, `INPUT_ENTRIES_NOT_CARRIED`                                   |
| `patch(*, section, patch)`                                        | `DocumentResult[PatchReport]`      | `JSON_PARSE_FAILED`, `PATCH_FAILED`                                                                |
| `insert_para(*, section, anchor, text, before=False)`             | `DocumentResult[StructuralReport]` | `PARAGRAPH_OUT_OF_RANGE`, `INPUT_ENTRIES_NOT_CARRIED`                                              |
| `delete_para(*, section, indexes)`                                | `DocumentResult[StructuralReport]` | `PARAGRAPH_OUT_OF_RANGE`, `INPUT_ENTRIES_NOT_CARRIED`                                              |
| `stamp(request, *, manifest=True)`                                | `DocumentResult[StampReport]`      | `INPUT_ENTRIES_NOT_CARRIED`, `ENCODE_SEMANTIC_LOSS`                                                |
| `restyle(*, preset)`                                              | `DocumentResult[RestyleReport]`    | `PRESET_NOT_FOUND`, `ENCODE_SEMANTIC_LOSS`                                                         |

Constructors and accessors beside these: `Document.open(path)` (bounded at 100 MB, raising
`INPUT_TOO_LARGE` beyond it), `Document.from_bytes(data)` (no bound — the caller already holds
the bytes), `to_bytes()`, `save(path)`, and `len()` / `==` / `hash()` over the bytes.

`set_cell`, `insert_para`, `delete_para` and `stamp` re-encode the whole package, so they
refuse a document Hancom saved with `INPUT_ENTRIES_NOT_CARRIED` or
`INPUT_NOT_ROUNDTRIP_SAFE` rather than drop the entries it carries. `fill` and `patch` work on
such a document.

### Module functions

| Function                                               | Returns                             | Characteristic failures |
| ------------------------------------------------------ | ----------------------------------- | ----------------------- |
| `convert_md(text, *, preset="default", base_dir=None)` | `DocumentResult[ConvertMdReport]`   | `PRESET_NOT_FOUND`      |
| `from_json(text, *, base=None)`                        | `DocumentResult[EncodeReport]`      | `JSON_PARSE_FAILED`     |
| `convert_hwp5(data, *, carry_layout_cache=False)`      | `DocumentResult[ConvertHwp5Report]` | `HWP5_DECODE_FAILED`    |
| `templates()`                                          | `TemplatesReport`                   | —                       |
| `schema(*, kind="document")`                           | `SchemaReport`                      | —                       |

Every report but `templates()` and `schema()` carries a `warnings` key, which is always
present and worth reading before saving the result.

## Documentation

- Guide: <https://ai-screams.github.io/HwpForge/guide/python.html>
- Project: <https://github.com/ai-screams/HwpForge>

## License

MIT OR Apache-2.0.
