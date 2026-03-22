# Examples

`examples/` is a curated showcase gallery for HwpForge outputs.

This directory is for:

- finished example artifacts that a human can open and inspect
- the Rust example source snapshot that generated those artifacts
- stable interop samples worth browsing as product examples

This directory is **not** for:

- local repro debris
- temporary comparison outputs
- issue-specific scratch files
- arbitrary conversion dumps that belong in `temp/`

Canonical generator sources still live under `crates/*/examples/`.
The `.rs` files colocated here are browsing-friendly snapshots bundled next to
the generated outputs.
Some snapshots still contain their original output paths (`temp/` or legacy
`examples/...`) because they are copied from the canonical generator source
without rewriting the implementation.

## Layout

```text
examples/
├── showcase/
│   ├── features/
│   ├── guides/
│   └── lists/
└── interop/
    ├── hwpx_json_roundtrip/
    └── hwpx_md_convert/
```

## Showcase

### Feature Isolation

Path:

- [`showcase/features/feature_isolation/`](showcase/features/feature_isolation)

What it contains:

- 15 focused HWPX outputs from `01_text.hwpx` through `15_shapes_advanced.hwpx`
- generator source snapshot: [`feature_isolation.rs`](showcase/features/feature_isolation/feature_isolation.rs)
- helper module snapshot: `feature_isolation_large/`

What to look for:

- one feature family per file
- compact visual verification of text, tables, images, links, equations, charts, and shapes

Bundle guide:

- [`showcase/features/feature_isolation/README.md`](showcase/features/feature_isolation/README.md)

### Guides

Complete guide bundle:

- output: [`hwpx_complete_guide.hwpx`](showcase/guides/hwpx_complete_guide/hwpx_complete_guide.hwpx)
- generator: [`hwpx_complete_guide.rs`](showcase/guides/hwpx_complete_guide/hwpx_complete_guide.rs)
- helper modules: `hwpx_complete_guide_parts/`
- bundle guide: [`README.md`](showcase/guides/hwpx_complete_guide/README.md)

Full report bundle:

- output: [`full_report.hwpx`](showcase/guides/full_report/full_report.hwpx)
- generator: [`full_report.rs`](showcase/guides/full_report/full_report.rs)
- bundle guide: [`README.md`](showcase/guides/full_report/README.md)

### Lists

Acceptance pack:

- bundle: [`showcase/lists/list_acceptance_visual/`](showcase/lists/list_acceptance_visual)
- generator: [`list_acceptance_visual.rs`](showcase/lists/list_acceptance_visual/list_acceptance_visual.rs)
- shared helper snapshot: `_support/list_visual.rs`
- bundle guide: [`README.md`](showcase/lists/list_acceptance_visual/README.md)

What to look for:

- bullet
- numbered
- outline
- checkable bullet
- continuation paragraphs
- mixed list transitions

## Interop

### HWPX JSON Roundtrip

Path:

- [`interop/hwpx_json_roundtrip/`](interop/hwpx_json_roundtrip)

What it contains:

- generator snapshot: [`hwpx_json_roundtrip.rs`](interop/hwpx_json_roundtrip/hwpx_json_roundtrip.rs)
- `hwpx2json/` outputs
- `json2hwpx/` outputs
- bundle guide: [`README.md`](interop/hwpx_json_roundtrip/README.md)

### HWPX Markdown Convert

Path:

- [`interop/hwpx_md_convert/`](interop/hwpx_md_convert)

What it contains:

- generator snapshot: [`hwpx_md_convert.rs`](interop/hwpx_md_convert/hwpx_md_convert.rs)
- `hwpx2md/` outputs
- extracted image assets under `hwpx2md/images/`
- bundle guide: [`README.md`](interop/hwpx_md_convert/README.md)

## Operational Notes

- HWP5 CLI conversion samples were moved out of `examples/`.
  They are operational conversion artifacts, not Rust example showcase bundles.
  See `temp/conversion/hwp5_to_hwpx/showcase_cli/` if you need those pairs.
- Visual experiments that are still under active investigation should stay in `temp/visual/`,
  not here.
