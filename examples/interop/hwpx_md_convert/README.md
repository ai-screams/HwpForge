# HWPX Markdown Convert

This bundle contains the HWPX → Markdown conversion showcase together with the
Rust generator snapshot.

Artifacts:

- generator snapshot: `hwpx_md_convert.rs`
- `hwpx2md/`: `01_text.hwpx`, `01_text.md`, `full_report.hwpx`, `full_report.md`, `hwpx_complete_guide.hwpx`, `hwpx_complete_guide.md`, and the mascot image `images/image1.png`

What to inspect:

- Markdown conversion of the generated HwpForge document
- the mascot image under `hwpx2md/images/`
- how document structure and media survive the conversion

Notes:

- `hwpx_md_convert.rs` is a copied snapshot of the canonical generator under
  `crates/hwpforge-smithy-md/examples/`.
- The snapshot is preserved as-is for browsing and may still reference its
  original output layout.
