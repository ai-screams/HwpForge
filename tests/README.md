# Tests Fixture Warehouse

`tests/` is primarily a shared fixture warehouse.
It is **not** the main Rust test crate layout.

## What lives here

```text
tests/
├── README.md
└── fixtures/
    ├── hwp5/
    ├── tables/
    ├── images/
    ├── charts/
    ├── mixed/
    ├── shapes/
    ├── layout/
    ├── fields/
    └── user_samples/
        ├── lists/
        ├── tabs/
        ├── tables/
        ├── text/
        └── user-authored and promoted research fixtures grouped by feature
```

## What does *not* live here

- the main unit/integration test source files
- a runnable `tests/` crate hierarchy
- arbitrary local conversion outputs that should have stayed in `temp/`

Actual tests run from:

- `crates/*/src/**` inline tests
- `crates/*/tests/*.rs` integration tests
- some crate `examples/*.rs` used as verification helpers

## Working rules

- before deleting a fixture, check whether code references it directly
- fixture filenames are hints, not truth; trust code/tests/parity checks first
- local repro artifacts should not accumulate here unless they are promoted into tracked regression inputs
