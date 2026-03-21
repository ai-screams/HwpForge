# MEMORY.md -- HwpForge Project Knowledge Base

> Last Updated: 2026-03-21
> Code-grounded snapshot. Prefer manifests, entrypoints, and shipped code over roadmap prose.

## Identity

| Field | Value |
| --- | --- |
| Name | HwpForge |
| Purpose | Programmatic control of Korean HWP/HWPX documents |
| Language | Rust (edition 2021) |
| License | MIT OR Apache-2.0 |
| Workspace version | `0.4.0` |
| MSRV / Dev Toolchain | `1.88 / 1.93` |
| Workspace packages | `10` |
| Tracked Rust `src` files under `crates/` | `137` |
| Tracked Rust `src` LOC under `crates/` | `83,962` |
| Example artifact files under `examples/` | `47` |
| GitHub workflow files | `5` |
| Coverage gate | 90%+ in CI |
| Design | Layered shared-IR architecture, warning-first for unknown semantics |

## Architecture

```text
Foundation
  -> Core
  -> Blueprint
  -> Smithy (HWPX / HWP5 / Markdown)
  -> Bindings (CLI / MCP / Python)
```

Key principle:

- Core carries shared document structure and style references.
- Blueprint carries style definitions and template resolution.
- Smithy crates do format-specific lowering and lifting.
- If HWP5 reveals semantics Core/HWPX cannot carry, shared-model work comes first.

## Workspace Packages

| Package | Role | Current state |
| --- | --- | --- |
| `hwpforge` | Umbrella facade crate | ACTIVE |
| `hwpforge-foundation` | Units, colors, indices, low-level primitives | SHIPPED |
| `hwpforge-core` | Shared document model | SHIPPED |
| `hwpforge-blueprint` | YAML template/style system | SHIPPED |
| `hwpforge-smithy-hwpx` | HWPX codec | SHIPPED |
| `hwpforge-smithy-md` | Markdown bridge | SHIPPED |
| `hwpforge-smithy-hwp5` | HWP5 reader / converter path | ACTIVE |
| `hwpforge-bindings-cli` | CLI | SHIPPED |
| `hwpforge-bindings-mcp` | MCP server | SHIPPED |
| `hwpforge-bindings-py` | Python bindings | STUB |

## Current Engineering State

- HWPX codec is shipped for read/write.
- Markdown bridge is shipped.
- HWP5 reader/converter line is active.
- CLI and MCP bindings are shipped.
- Python bindings remain a stub.
- Shared tab semantics already landed on `main`.
- Local `feat/list-shared-semantics` now includes shared `ordered / bullet / outline` semantics across `core -> blueprint -> smithy-hwpx`, with Markdown bridge integration.
- Local `feat/list-shared-semantics` also includes `CheckBullet` semantics and Markdown task-list normalization.
- HWP5 is still the partial leg for checkable support:
  - definition-level parity exists
  - paragraph-level checked item state decode remains backlog
- Current local workstream is still `feat/list-shared-semantics`, but the remaining work is no longer "list exists or not" and is now mostly parity/backlog clean-up.

## Current Local Workstream

Delivered on local branch:

- shared `ordered / bullet / outline` semantics
- `CheckBullet` as a separate list semantics variant
- HWPX checkable bullet encode/decode wiring
- Markdown task list -> `CheckBullet` normalization
- ordered Markdown task list -> unordered checkable normalization policy

Remaining structural backlog:

- HWP5 paragraph-level checked item state reverse-trace / decode
- mixed ordered-task list list-scope normalization
- custom checked glyph authoring fixture / policy

Verified local research fixtures:

- `tests/fixtures/user_samples/lists/sample-numbered-list-custom-formats.{hwp,hwpx}`
- `tests/fixtures/user_samples/lists/sample-numbered-list-multilevel.{hwp,hwpx}`
- `tests/fixtures/user_samples/lists/sample-mixed-lists-with-outline.{hwp,hwpx}`

Working rules for this slice:

- `Paragraph.heading_level` is not list semantics
- HWPX list semantics live on `paraPr/heading(type,idRef,level)`
- checkable bullet in HWPX is still `heading(type="BULLET")`, not a new kind
- `bullet.checkedChar` is definition-level; `paraPr.checked` is item-level
- bullet `level` and bullet glyph choice are different axes; glyph switching requires explicit `bullet_id` choice
- Markdown task lists are HWPX-first normalization input, not a promise of full Markdown semantics fidelity
- public API / semver breaking changes require explicit user approval before implementation
- `.docs/` is local planning memory by default and should not be committed unless explicitly requested

## CI / Release Operations

- `ci.yml` handles contributor verification
- `release-plz.yml` owns release PR and publish flow
- `pages.yml` handles Pages build/deploy
- `security.yml` handles scheduled advisory scans
- `make ci-fast` is the default fast gate
- `make ci-full` adds coverage and MSRV checks
- `make ci` is an alias of `ci-fast`

## Critical Gotchas

- Color is BGR internally, not RGB.
- `1pt = 100 HwpUnit`; avoid hand-rolled float math.
- HWPX geometry uses `hc:` namespace, not `hp:`.
- HWPX XML element order is semantic; serde field order matters.
- Chart XML parts are ZIP-only and must not be listed in `content.hpf`.
- TextBox is `rect + drawText`, not a generic control bucket.
- `Paragraph.heading_level` and HWPX `paraPr/heading(type,idRef,level)` are different axes.
- `sample-mixed-lists-with-outline` is the current same-section coexistence fixture for bullet/number/restart/outline.
- HWPX checkable bullet is `BULLET` + `checkedChar` + `paraHead.checkable` + `paraPr.checked`; miss one and support is fake.
- `CheckBullet` is not `Bullet + bool`; item state and definition state live on different layers.
- Markdown ordered task lists are intentionally normalized to unordered checkable semantics because shared HWP semantics do not yet prove a numbered-checkable kind.
- Multi-paragraph Markdown task items are bridge-layer continuation logic, not a new HWP/HWPX list family.
- HWP5 field IDs diverge in the wild; alias normalization + unknown preservation remains policy.
- HWP5 table page-break truth for current controlled fixtures is `0=None`, `1=Table`, `2=Cell`.
- HWP5 checkable support is partial parity only until paragraph-level checked item state is traced.
- Silent normalization of unknown semantics is forbidden. Warning-first or explicit failure.

## Documentation Map

| Path | Purpose |
| --- | --- |
| `AGENTS.md` | Root agent guidance and current repo facts |
| `CLAUDE.md` | Claude-specific repo guidance |
| `MEMORY.md` | This current project snapshot |
| `crates/AGENTS.md` | Crate-layer guidance |
| `crates/*/AGENTS.md` | Crate-local gotchas and responsibilities |
| `.docs/planning/ROADMAP.md` | Current roadmap snapshot |
| `.docs/planning/2026-03-20-list-shared-semantics-plan.md` | Current list implementation plan |
| `.docs/research/2026-03-20_list_shared_semantics_handoff.md` | Current list handoff note |
| `.docs/research/hwp5/HWP_LIST_STRUCTURE_RELATION_TREE_2026-03-21.md` | HWP/HWPX list relation tree + post-implementation gotchas |
| `.docs/research/` | Local research logs |
| `.docs/references/openhwp/docs/hwpx/` | Local HWPX reference set |

## Dev Commands

```bash
make test        # cargo nextest run --workspace --all-features
make test-ci     # nextest CI profile
make clippy      # cargo clippy --workspace --all-targets --all-features -- -D warnings
make fmt         # cargo fmt --all -- --check
make deny        # cargo deny --all-features check
make doc         # cargo doc --workspace --all-features --no-deps --open
make msrv        # cargo +1.88 check --workspace --all-features
make ci-fast     # fmt + clippy + test + deny + lint-md
make ci-full     # ci-fast + coverage + msrv
```

## Operational Notes

- Root `tests/` is mostly a fixture warehouse, not a Rust integration-test crate.
- `examples/` is artifact/output space, not primary source code.
- `.docs/` can drift outside git tracking in this repo workflow.
- Do not claim support is complete from enum names or partial wire handling alone.
- If a feature needs shared semantics, implement the shared IR first and lower it later.
