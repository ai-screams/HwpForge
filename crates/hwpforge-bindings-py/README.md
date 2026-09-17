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
unpacked and imported without pip. On Debian 12 (glibc 2.36) or anything newer:

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

Every wheel and the source distribution are attached to the matching
[GitHub Release](https://github.com/ai-screams/HwpForge/releases), so an air-gapped host
can be served from there as well as from PyPI.

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
