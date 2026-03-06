# HwpForge

> **Programmatic control of Korean HWP/HWPX documents in Rust**
>
> Read, write, and convert [Hancom](https://www.hancom.com/) 한글 files

[![CI](https://github.com/ai-screams/HwpForge/actions/workflows/ci.yml/badge.svg)](https://github.com/ai-screams/HwpForge/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![crates.io](https://img.shields.io/crates/v/hwpforge.svg)](https://crates.io/crates/hwpforge)
[![docs.rs](https://docs.rs/hwpforge/badge.svg)](https://docs.rs/hwpforge)

---

## What is HwpForge?

HwpForge is a pure-Rust library for working with HWPX documents (ZIP + XML, KS X 6101) used by modern versions of Hancom 한글, the dominant word processor in Korea. It provides:

- **Full HWPX codec** -- decode and encode HWPX files with lossless roundtrip
- **Markdown bridge** -- convert between GFM Markdown and HWPX
- **YAML style templates** -- reusable design tokens (like Figma) for fonts, sizes, colors
- **Type-safe API** -- branded indices, typestate validation, zero unsafe code

## Quick Start

Add to your `Cargo.toml`:

```toml
[dependencies]
hwpforge = "0.1"
```

### Build a document

```rust
use hwpforge::core::{Document, Draft, Paragraph, Run, Section, PageSettings};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};

let mut doc = Document::<Draft>::new();
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::text("Hello, 한글!", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));
```

### Encode to HWPX

```rust
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::core::ImageStore;

let validated = doc.validate().unwrap();
let style_store = HwpxStyleStore::default_modern();
let image_store = ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
std::fs::write("output.hwpx", &bytes).unwrap();
```

### Decode from HWPX

```rust
use hwpforge::hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("input.hwpx").unwrap();
println!("Sections: {}", result.document.sections().len());
```

### Markdown to HWPX

```rust
use hwpforge::md::MdDecoder;
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};

let md_doc = MdDecoder::decode("# Title\n\nHello from Markdown!").unwrap();
let validated = md_doc.document.validate().unwrap();
let style_store = HwpxStyleStore::from_registry(&md_doc.registry);
let image_store = hwpforge::core::ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
```

## Feature Flags

| Feature | Default | Description                  |
| ------- | ------- | ---------------------------- |
| `hwpx`  | Yes     | HWPX encoder/decoder         |
| `md`    | --      | Markdown <-> Core conversion |
| `full`  | --      | All features                 |

```toml
# Markdown support
hwpforge = { version = "0.1", features = ["full"] }
```

## Supported Content

| Category        | Elements                                                                       |
| --------------- | ------------------------------------------------------------------------------ |
| Text            | Runs, character shapes, paragraph shapes, styles (22 Hancom defaults)          |
| Structure       | Tables (nested), images (binary + path), text boxes, captions                  |
| Layout          | Multi-column, page settings, landscape, gutter, master pages                   |
| Headers/Footers | Header, footer, page numbers (autoNum)                                         |
| Notes           | Footnotes, endnotes                                                            |
| Shapes          | Line, ellipse, polygon, arc, curve, connect line (with fill, rotation, arrows) |
| Equations       | HancomEQN script format                                                        |
| Charts          | 18 chart types (OOXML-compatible)                                              |
| References      | Bookmarks, cross-references, fields (date/time/summary), memos, index marks    |
| Annotations     | Dutmal (side text), compose characters                                         |
| Markdown        | GFM decode, lossy + lossless encode, YAML frontmatter                          |

## Architecture

```
hwpforge (umbrella crate)
  |
  +-- hwpforge-foundation    Primitives: HwpUnit, Color (BGR), Index<T>
  +-- hwpforge-core          Document model: Section, Paragraph, Table, Shape
  +-- hwpforge-blueprint     YAML style templates with inheritance
  +-- hwpforge-smithy-hwpx   HWPX codec (ZIP+XML, KS X 6101)
  +-- hwpforge-smithy-md     Markdown codec (GFM + frontmatter)
```

**Key principle**: Structure and style are separate (like HTML + CSS).
Core holds document structure with style _references_ (indices).
Blueprint holds style _definitions_ (fonts, sizes, colors).
Smithy compilers fuse Core + Blueprint into the target format.

## Stats

| Metric          | Value                 |
| --------------- | --------------------- |
| Total LOC       | ~49,200               |
| Tests           | 1,510 (cargo-nextest) |
| Source files    | 92 .rs                |
| Crates          | 9 (6 publishable)     |
| Coverage        | 92.65%                |
| Clippy warnings | 0                     |
| Unsafe code     | 0                     |

## Development

### Prerequisites

- Rust 1.88+ (MSRV)
- (Recommended) [cargo-nextest](https://nexte.st/) for parallel testing
- (Optional) [pre-commit](https://pre-commit.com/) for git hooks

### Commands

```bash
make ci          # fmt + clippy + test + deny + lint (matches CI exactly)
make test        # cargo nextest run
make clippy      # cargo clippy (all targets, all features, -D warnings)
make fmt-fix     # auto-format with rustfmt
make doc         # generate rustdoc (opens browser)
make cov         # coverage report (90% gate)
```

### Project Structure

```
HwpForge/
├── crates/
│   ├── hwpforge/                 # Umbrella crate (re-exports)
│   ├── hwpforge-foundation/      # Primitives (HwpUnit, Color, Index<T>)
│   ├── hwpforge-core/            # Document model (style refs only)
│   ├── hwpforge-blueprint/       # YAML templates (Figma-like)
│   ├── hwpforge-smithy-hwpx/     # HWPX codec (ZIP+XML <-> Core)
│   ├── hwpforge-smithy-md/       # Markdown codec (MD <-> Core)
│   ├── hwpforge-smithy-hwp5/     # HWP5 decoder (planned)
│   ├── hwpforge-bindings-py/     # Python bindings (planned)
│   └── hwpforge-bindings-cli/    # CLI tool (planned)
├── tests/                        # Integration tests + golden fixtures
└── examples/                     # Usage examples
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Acknowledgments

This project references the public documentation for Hancom 한글 (.hwp/.hwpx) file formats.
The HWPX format follows the KS X 6101 (OWPML) national standard.
