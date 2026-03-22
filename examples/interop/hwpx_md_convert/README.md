# HWPX Markdown Convert

This bundle contains the HWPX → Markdown conversion showcase together with the
Rust generator snapshot.

Artifacts:

- generator snapshot: `hwpx_md_convert.rs`
- `hwpx2md/`: input HWPX files, generated Markdown, and extracted image assets

What to inspect:

- Markdown conversion results for small and large HWPX inputs
- extracted images under `hwpx2md/images/`
- how document structure and media survive the conversion

Notes:

- `hwpx_md_convert.rs` is a copied snapshot of the canonical generator under
  `crates/hwpforge-smithy-md/examples/`.
- The snapshot is preserved as-is for browsing and may still reference its
  original output layout.
