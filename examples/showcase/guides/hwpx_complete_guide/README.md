# HWPX Complete Guide

This bundle contains the complete guide-style showcase document and the Rust
generator snapshot used to build it.

Artifacts:

- `hwpx_complete_guide.hwpx`
- generator snapshot: `hwpx_complete_guide.rs`
- helper snapshot: `hwpx_complete_guide_parts/`

What to inspect:

- a broad tour of HWPX authoring capabilities in one polished document
- how the guide generator composes multiple helper modules into one output

Notes:

- The bundled `.rs` and helper modules are copied snapshots of the canonical
  sources under `crates/hwpforge-smithy-hwpx/examples/`.
- They keep their original output-path assumptions for traceability.
