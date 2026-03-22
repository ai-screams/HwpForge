# HWPX JSON Roundtrip

This bundle contains the browsing-friendly outputs for the HWPX ↔ JSON
roundtrip example together with the Rust generator snapshot.

Artifacts:

- generator snapshot: `hwpx_json_roundtrip.rs`
- `hwpx2json/`: original HWPX copied next to exported JSON
- `json2hwpx/`: JSON imported back into HWPX

What to inspect:

- how document + style data is serialized into JSON
- which example inputs are used as roundtrip anchors
- what the restored HWPX output looks like after re-import

Notes:

- `hwpx_json_roundtrip.rs` is a copied snapshot of the canonical generator
  under `crates/hwpforge-smithy-hwpx/examples/`.
- The snapshot still refers to the legacy `examples/...` paths in its
  implementation. The colocated outputs here are for browsing, not for editing
  the canonical generator in place.
