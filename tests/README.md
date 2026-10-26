# Shared test inputs

Rust tests live in `crates/*/src` and `crates/*/tests`; Python tests live in
`crates/hwpforge-bindings-py/tests`.

`tests/fixtures` contains encoder-generated HWPX inputs, text inputs and image
assets shared by those tests:

- `fields/`: named fields for reading and filling.
- `layout/`: layout cache validation.
- `stamp/`: placeholder detection and stamping.
- `structural/`: paragraph editing.
- `tables/`: table reading and cell editing.
- `markdown/`: Markdown conversion inputs.
- `images/`: image assets and rendering expectations.

Before changing an input, check its consumers and preserve their error and
warning contracts. Run `make fixture-policy` to check binary document metadata.
