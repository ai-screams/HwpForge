# hwpforge

Tongs — Read, edit and generate Korean HWP/HWPX documents from Python.

> 본 제품은 한컴의 HWP 문서 파일(.hwp) 공개 문서를 참고하여 개발하였습니다.
> (Developed with reference to Hancom's public HWP file format documents: <https://www.hancom.com/etc/hwpDownload.do>.)

`hwpforge` wraps the [HwpForge](https://github.com/ai-screams/HwpForge) Rust library: a
document model for the HWPX (OWPML) format, an HWP5 reader, a Markdown bridge and a PDF
renderer. The Python package is a thin layer over that library, so it performs the same
operations, with the same meaning, as the command line tool and the MCP server. What each
frontend calls them differs: method names, argument spellings and some failure codes are
Python's own public contract, and the other two keep theirs.

## What it reads and writes

Reads `.hwpx` and `.hwp` (HWP5) through supported document operations.
Conversion is one way: editing produces HWPX. See the input and loss policy below.

Writes `.hwpx` only. There is no `.hwp` output, and Hancom Office opens `.hwpx` natively.
Documents also export to Markdown, JSON and PDF. A PDF needs the document's own fonts present
on the host and named through `font_dirs` or `discovery`: by default a face that cannot be
resolved fails the render rather than being guessed at. `to_pdf(degraded=True)` relaxes that
and renders the missing face with a fallback, which changes how the page looks.

Rendering replays a layout stored in the document, so `to_pdf` needs a document that carries
one. Two do: an HWPX Hancom saved, and an HWPX converted from HWP5, which carries the layout
across by default — `hwpforge.convert_hwp5(data).document.to_pdf(...)`. A document
this library generated from Markdown or JSON carries no layout and is refused with
`PDF_RENDER_FAILED`.

A paragraph whose line starts cannot be carried over exactly loses its layout and is reported
as `LAYOUT_CACHE_DROPPED`; `convert_hwp5(data, carry_layout_cache=False)` leaves the layout out.

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
that has to decode the document first can also raise `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX).
An operation that edits an HWPX document also raises `DECODE_FAILED` when the package carries a digital signature or encryption marker, because such a document cannot be edited.

### `Document` methods

| Method                                                            | Returns                            | Characteristic failures                                                                                             |
| ----------------------------------------------------------------- | ---------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `inspect(*, styles=False)`                                        | `InspectReport`                    | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `outline()`                                                       | `OutlineReport`                    | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `fields()`                                                        | `FieldsReport`                     | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `validate()`                                                      | `ValidateReport`                   | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX) (an invalid document reports, not raises) |
| `read(*, section, paras, table, field)`                           | `ReadReport`                       | `READ_TARGET_REQUIRED`, `READ_PARAS_INVALID`, `READ_PARA_RANGE_INVALID`, `TABLE_NOT_FOUND`                          |
| `diff(revised)`                                                   | `DiffReport`                       | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `stamp_plan()`                                                    | `StampPlanReport`                  | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `to_json(*, styles=True)`                                         | `TextResult[ToJsonReport]`         | `HWP5_DECODE_FAILED` (malformed HWP5) or `DECODE_FAILED` (malformed HWPX)                                           |
| `export_section(*, section, styles=True)`                         | `TextResult[ExportSectionReport]`  | `SECTION_OUT_OF_RANGE`                                                                                              |
| `to_md(*, mode="styled")`                                         | `TextResult[ToMdReport]`           | `ENCODE_FAILED` (`mode="lossless"`: see the note below)                                                             |
| `to_pdf(*, font_dirs, discovery, degraded, partial_cache_reject)` | `BytesResult[ToPdfReport]`         | `PDF_RENDER_FAILED`                                                                                                 |
| `fill(values)`                                                    | `DocumentResult[FillReport]`       | `FIELD_NOT_FOUND`, `FIELD_NOT_FILLABLE`, `EMPTY_FIELD_VALUE`                                                        |
| `set_cell(*, table, at, right_of, below, text, specs)`            | `DocumentResult[SetCellReport]`    | `TABLE_NOT_FOUND`, `CELL_NOT_FOUND`, `INPUT_ENTRIES_NOT_CARRIED`                                                    |
| `patch(*, section, patch)`                                        | `DocumentResult[PatchReport]`      | `JSON_PARSE_FAILED`, `PATCH_FAILED`, `PROTECTED_FIELD_RANGE`                                                        |
| `insert_para(*, section, anchor, text, before=False)`             | `DocumentResult[StructuralReport]` | `PARAGRAPH_OUT_OF_RANGE`, `INPUT_ENTRIES_NOT_CARRIED`                                                               |
| `delete_para(*, section, indexes)`                                | `DocumentResult[StructuralReport]` | `PARAGRAPH_OUT_OF_RANGE`, `INPUT_ENTRIES_NOT_CARRIED`                                                               |
| `stamp(request, *, manifest=True)`                                | `DocumentResult[StampReport]`      | `INPUT_ENTRIES_NOT_CARRIED`, `ENCODE_SEMANTIC_LOSS`                                                                 |
| `restyle(*, preset)`                                              | `DocumentResult[RestyleReport]`    | `PRESET_NOT_FOUND`, `ENCODE_SEMANTIC_LOSS`                                                                          |

`to_md(mode="lossless")` keeps what the preservation scope lists, raises `ENCODE_FAILED` for
an object the scope lists as refused, and may leave out anything else (paragraph styles,
headers and footers …) without an error — the scope is the "Lossless 보존 범위" section of the
[Markdown bridge guide](https://ai-scream.ai/HwpForge/guide/markdown-bridge.html#lossless-보존-범위).
The lossless text is a machine-oriented view for checking that scope: `convert_md` refuses it
(`MD_DECODE_FAILED`), and round-trip edits go through `to_json`/`from_json`.

Constructors and accessors beside these: `Document.open(path)` (bounded at 100 MB, raising
`INPUT_TOO_LARGE` beyond it), `Document.from_bytes(data)` (no bound — the caller already holds
the bytes), `to_bytes()`, `save(path)`, and `len()` / `==` / `hash()` over the bytes.

`set_cell`, `insert_para` and `stamp` re-encode the whole package, so they
refuse a document Hancom saved with `INPUT_ENTRIES_NOT_CARRIED` or
`INPUT_NOT_ROUNDTRIP_SAFE` rather than drop the entries it carries. `fill` and `patch` work on
such a document. `delete_para` also supports verified plain or empty top-level body
paragraphs in Hancom-saved HWPX, using input indexes. Its source-preserving path keeps
surviving paragraph IDs and unrelated payloads, removes affected line caches, zeroes
supported affected table heights, and removes Preview and its connections with
`PREVIEW_REMOVED`. Hancom lays out the document again when it is opened.
Saving it again in Hancom rebuilds Preview.
An unverifiable deletion returns the original admission error and no bytes.

### Module functions

| Function                                               | Returns                             | Characteristic failures |
| ------------------------------------------------------ | ----------------------------------- | ----------------------- |
| `convert_md(text, *, preset="default", base_dir=None)` | `DocumentResult[ConvertMdReport]`   | `PRESET_NOT_FOUND`      |
| `from_json(text, *, base=None)`                        | `DocumentResult[EncodeReport]`      | `JSON_PARSE_FAILED`     |
| `convert_hwp5(data, *, carry_layout_cache=True)`       | `DocumentResult[ConvertHwp5Report]` | `HWP5_DECODE_FAILED`    |
| `templates()`                                          | `TemplatesReport`                   | —                       |
| `schema(*, kind="document")`                           | `SchemaReport`                      | —                       |

Every report but `templates()` and `schema()` carries a `warnings` key, which is always
present and worth reading before saving the result.

## Documentation

- Guide: <https://ai-scream.ai/HwpForge/guide/python.html>
- Project: <https://github.com/ai-screams/HwpForge>

## License

MIT OR Apache-2.0.

### 0.18 error migration

Operation codes now use the canonical `OpsCode` vocabulary across Python, CLI and MCP. Read target failures use `SECTION_OUT_OF_RANGE`, `TABLE_NOT_FOUND` and `FIELD_NOT_FOUND`.

File I/O raises `HwpForgeIOError` (`HwpForgeError` and `OSError`), or `HwpForgeFileNotFoundError` for an absent input (also `FileNotFoundError`). Codes are `FILE_READ_FAILED`, `FILE_WRITE_FAILED`, and `FILE_NOT_FOUND`; the size cap remains `INPUT_TOO_LARGE`. OS fields and copy/pickle reconstruction are preserved. Exact type checks and `except PermissionError` need updating. `Document.save` to a missing parent directory now raises `HwpForgeIOError` (`FILE_WRITE_FAILED`), not `FileNotFoundError`. Change `except FileNotFoundError` around `save` to `except HwpForgeIOError` or `except OSError`. Install matching native and Python package versions.

## HWP5 input and loss acceptance

`Document.open("form.hwp")` and `Document.from_bytes(data)` accept HWP5 input. `inspect`, `outline`, `read`, `fields`, `validate`, `diff` (both inputs), `stamp_plan`, `to_md`, `to_json`, `export_section`, and the five edits convert through the shared HWP5-to-HWPX path. Outputs are HWPX.

`fill`, `set_cell`, `insert_para`, `delete_para`, and `stamp` default to `accept_conversion_loss=False`. Semantic conversion loss raises `HWP5_CONVERSION_LOSS` with rejected warnings in `error.details["warnings"]`. Review them before explicitly passing `accept_conversion_loss=True`. The order is conversion, existing operation, then conversion-loss refusal; existing editing admission checks remain in force. Reads report warnings.

Existing `warnings` are unchanged. HWP5 input adds a separate report key: `conversion = {"source_format": "hwp5", "warnings": [...]}`. Each warning has `stage`, `code`, `message`, and optional `location`. Diff reports converted roles under `conversion.base` and `conversion.revised`. JSON and Markdown payloads contain no conversion metadata.

`save` returns `None`. After a successful write, a save that converts raw HWP5 emits its conversion warnings, including accepted loss and non-loss warnings, as standard `UserWarning`s with the code and any known location. Python's warning filters apply: the default filter shows repeats once, and an `error` filter raises after the file is written. For the full list, use the `conversion` report of a read or edit, or `warnings.catch_warnings(record=True)` with `warnings.simplefilter("always")` inside it.

`save` rejects `.hwp` (case insensitive) with `INVALID_EXTENSION`. Saving raw HWP5 converts to HWPX and also requires `accept_conversion_loss=True` if lossy. The identity of the file read by `Document.open` is retained through instance edits. Saving over that HWP5 file, including symlink and hard-link aliases, raises `INVALID_OUTPUT_PATH` even after its original path is deleted or replaced. Protection applies to the file read at open time, not the file currently at that path. The file is identified by device and inode, so a new file that received the deleted source's inode is refused too; save to another path in that case. Only the output identity is looked up when saving; lookup failures other than an absent output raise `FILE_WRITE_FAILED`. New targets and HWPX in-place saves remain allowed. Output paths must remain stable while saving.

Bytes-only constructors and the generating functions `convert_hwp5`, `convert_md`, and `from_json` have no source file identity to protect; callers must keep outputs separate from their original files. Bytes/text results perform no writes. Automatic HWP5 conversion for `from_json(base=...)`, `patch`, and `restyle` is out of scope: they still raise `HWP5_INPUT`; explicitly convert their input first.
