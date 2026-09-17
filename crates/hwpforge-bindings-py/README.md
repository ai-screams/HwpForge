# hwpforge

Read, edit and generate Korean HWP/HWPX documents from Python.

`hwpforge` wraps the [HwpForge](https://github.com/ai-screams/HwpForge) Rust library: a
document model for the HWPX (OWPML) format, an HWP5 reader, a Markdown bridge and a PDF
renderer. The Python package is a thin layer over that library, so the operations, their
option names and their error codes are the same ones the command line tool and the MCP
server use.

## Install

```console
pip install hwpforge
```

Wheels are published for CPython 3.9 and newer (a single `abi3` wheel per platform) on
Linux x86_64/aarch64, macOS arm64/x86_64 and Windows x64. Free-threaded builds are not
supported yet. Building from the source distribution needs Rust 1.92 and maturin.

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

## Documentation

- Guide: <https://ai-screams.github.io/HwpForge/guide/python.html>
- Project: <https://github.com/ai-screams/HwpForge>

## License

MIT OR Apache-2.0.
