# MEMORY.md -- HwpForge Project Knowledge Base

> Last Updated: 2026-02-20
> Sections marked [stable] rarely change; [volatile] updates with each phase.

## Identity [stable]

| Field     | Value                                 | Field            | Value                 |
| --------- | ------------------------------------- | ---------------- | --------------------- |
| Name      | HwpForge                              | License          | MIT OR Apache-2.0     |
| Purpose   | HWP/HWPX Korean doc control           | Language         | Rust (edition 2021)   |
| Version   | 0.1.0                                 | MSRV / Toolchain | 1.85 / 1.93 stable    |
| Workspace | 8 crates                              | LOC / Tests      | ~37,000 / 988 passing |
| Design    | Clean Room (ideas only, no code copy) | Coverage         | 90%+ (CI-enforced)    |

## Architecture [stable]

```
Foundation (HwpUnit, Color, IDs) -> Core (document DOM) -> Blueprint (YAML styles)
    -> Smithy (HWPX/HWP5/MD compilers) -> Bindings (Python, CLI, MCP)
```

Key: **Structure** (Core) vs **Style** (Blueprint) separation -- like HTML+CSS.

---

## Crate Status [volatile]

| Crate                 | Phase         | Tests |    LOC | Status      |
| --------------------- | ------------- | ----: | -----: | ----------- |
| hwpforge-foundation   | Phase 0       |   185 |  4,432 | COMPLETE    |
| hwpforge-core         | Phase 1+4.5   |   291 |  6,452 | COMPLETE    |
| hwpforge-blueprint    | Phase 2       |   191 |  4,647 | COMPLETE    |
| hwpforge-smithy-hwpx  | Phase 3-4+4.5 |   246 | 13,076 | COMPLETE    |
| hwpforge-smithy-md    | Phase 5       |    73 |  3,779 | COMPLETE    |
| hwpforge-smithy-hwp5  | Phase 10      |     1 |     14 | STUB (v2.0) |
| hwpforge-bindings-py  | Phase 6       |     1 |     14 | STUB        |
| hwpforge-bindings-cli | Phase 6       |     0 |      3 | STUB        |

Phase 4.5 features (all complete): ImageStore, HeaderFooter, PageNumber,
Footnote, Endnote, TextBox, Multi-column, Shapes (Line/Ellipse/Polygon),
Caption, ShapeStyle, Equation, Chart.

---

## Roadmap [volatile]

| Phase | Description            | Status   |
| ----: | ---------------------- | -------- |
|     0 | Foundation             | DONE     |
|     1 | Core DOM               | DONE     |
|     2 | Blueprint styles       | DONE     |
|   3-4 | Smithy-HWPX (R/W)      | DONE     |
|   4.5 | Extended elements W1-6 | DONE     |
|     5 | Smithy-MD              | DONE     |
|     6 | Bindings (Python+CLI)  | **NEXT** |
|     7 | MCP Integration        | PLANNED  |
|     8 | Testing + Release v1.0 | PLANNED  |
|    10 | HWP5 Reader (v2.0)     | DEFERRED |

SSoT: `.docs/planning/ROADMAP.md`

---

## CI/CD [stable]

Fan-out Gate Pipeline (`ci.yml` -- reusable via `workflow_call`):

- **Tier 1 (Gate)**: lint-format + lint-clippy
- **Tier 2 (Verify)**: test, coverage (90%), deny, lint-docs, msrv (1.85)
- **Tier 3 (Platform)**: cross-platform (Windows, macOS)

Triggers: push(main), PR, merge_group, schedule(weekly), workflow_call.
Release: `release.yml` -> `ci.yml`(full) -> build -> GitHub Release.
Security: cargo-deny on PR + weekly advisory scan.
Pre-commit: markdownlint + dprint + file hygiene (no Rust checks for speed).

---

## Critical Gotchas [stable]

These are hard-won lessons. Violating any will cause subtle bugs.

**Color**: BGR internally, NOT RGB. Red = `0x0000FF`.

**Units**: 1pt = 100 HwpUnit, 1mm = 283 HwpUnit (approx).

**HWPX Shapes**: Geometry uses `hc:` namespace (`<hc:startPt>`), NOT `hp:`.

**Line element**: Field order matters -- geometry (startPt/endPt) BEFORE sizing (sz/pos/outMargin).

**Equations**: HancomEQN script format, NOT MathML. Syntax: `{a+b} over {c+d}`.
No shape common block. Always inline (`treatAsChar="1"`, `flowWithText="1"`).

**Charts**: OOXML `<c:chartSpace>` format. Bar vs Column = same `<c:barChart>` with `barDir` attribute.
Chart XMLs live in `Chart/chartN.xml` within ZIP but are NOT listed in `content.hpf` manifest.

**HWP5 field IDs**: Diverge by implementation (Date: `%dte/$dte/%dat`).
Parser policy: alias normalization + unknown ID preservation (no hard-fail).

**CI**: `dependency-review-action` requires GHAS (unavailable on private repos).
`workflow_call` sets `github.event_name` to `workflow_call`, not the caller's event.

## Key Decisions [stable]

- **TDD**: Edge cases first, normal cases last
- **YAGNI**: No speculative features
- **Minimal deps**: quick-xml 0.36, serde 1.0, zip 2.1, thiserror 2.0, pulldown-cmark 0.12
- **100% rustdoc**: All public APIs documented

## Documentation Map [stable]

| Path                          | Purpose                                   |
| ----------------------------- | ----------------------------------------- |
| `AGENTS.md` (root + 8 crates) | AI agent context (hierarchical)           |
| `MEMORY.md`                   | This file -- centralized knowledge        |
| `.docs/planning/ROADMAP.md`   | Roadmap SSoT                              |
| `.docs/architecture/`         | Type system, crate roles, TDD             |
| `.docs/` (git-excluded)       | Internal research, not version-controlled |

## Dev Commands [stable]

```
cargo nextest run --workspace    # Test        make ci       # Full CI
cargo clippy --workspace         # Lint        make ci-full  # CI + coverage + MSRV
bacon                            # Watch mode
```
