# MEMORY.md -- HwpForge Project Knowledge Base

> Last Updated: 2026-03-15
> Sections marked [stable] rarely change; [volatile] should be re-verified after major feature or workflow changes.

## Identity [stable]

| Field              | Value                                             | Field                | Value               |
| ------------------ | ------------------------------------------------- | -------------------- | ------------------- |
| Name               | HwpForge                                          | License              | MIT OR Apache-2.0   |
| Purpose            | Programmatic control of Korean HWP/HWPX documents | Language             | Rust (edition 2021) |
| Version            | 0.1.7                                             | MSRV / Dev Toolchain | 1.88 / 1.93         |
| Workspace          | 10 Cargo packages                                 | Tracked Rust LOC     | ~62,664             |
| Tests              | 1,881 nextest runnable                            | Coverage Gate        | 90%+ in CI          |
| Tracked Rust Files | 144                                               | Unsafe               | Forbidden           |
| Design             | Clean Room (ideas only, no code copy)             | CI workflows         | 5                   |

## Architecture [stable]

```
Foundation (HwpUnit, Color, IDs)
  -> Core (document DOM, style references only)
  -> Blueprint (YAML styles and style registry)
  -> Smithy (HWPX / Markdown compilers, HWP5 reader/converter)
  -> Bindings (CLI / MCP shipped, Python planned)
```

Key principle: **Structure** (Core) and **Style** (Blueprint) stay separate, like HTML + CSS.

---

## Workspace Packages [volatile]

| Package                 | Role                                         | Status            |
| ----------------------- | -------------------------------------------- | ----------------- |
| `hwpforge`              | Umbrella facade crate (`hwpx`, `md`, `full`) | ACTIVE            |
| `hwpforge-foundation`   | Primitives, units, colors, branded indices   | COMPLETE          |
| `hwpforge-core`         | Format-agnostic document model               | COMPLETE          |
| `hwpforge-blueprint`    | YAML template/style system                   | COMPLETE          |
| `hwpforge-smithy-hwpx`  | HWPX decoder/encoder                         | COMPLETE          |
| `hwpforge-smithy-md`    | Markdown bridge                              | COMPLETE          |
| `hwpforge-smithy-hwp5`  | HWP5 reader / converter path                 | ACTIVE / Phase 10 |
| `hwpforge-bindings-cli` | CLI entrypoint                               | SHIPPED           |
| `hwpforge-bindings-mcp` | MCP server                                   | SHIPPED           |
| `hwpforge-bindings-py`  | Python bindings                              | STUB / planned    |

Write-path completion status:

- Phase 0-5 complete
- Phase 4.5 Wave 1-6 complete
- Phase 5.5 complete
- Wave 7-14 complete

---

## Roadmap Snapshot [volatile]

|     Phase | Description                        | Status      |
| --------: | ---------------------------------- | ----------- |
|         0 | Foundation                         | DONE        |
|         1 | Core DOM                           | DONE        |
|         2 | Blueprint styles                   | DONE        |
|       3-4 | Smithy-HWPX read/write             | DONE        |
|       4.5 | Extended write features (Wave 1-6) | DONE        |
|         5 | Smithy-MD                          | DONE        |
|       5.5 | Zero-config write API              | DONE        |
| Wave 7-14 | Remaining HWPX write surface       | DONE        |
|         6 | CLI bindings                       | DONE        |
|         7 | MCP server / distribution          | DONE        |
|         8 | Testing + release hardening        | PLANNED     |
|        10 | HWP5 reader / converter            | IN PROGRESS |

Planning sources:

- `.docs/planning/ROADMAP.md`
- `.docs/research/hwp5/HWP5_RESEARCH_EXECUTION_BLUEPRINT.md`
- `.docs/research/hwp5/HWP5_RESEARCH_MASTER.md`

---

## CI/CD [stable]

Purpose-based workflows:

- **`ci.yml`**: contributor verification for `pull_request`, `merge_group`, `workflow_dispatch`, `workflow_call`
- **Gate jobs**: format + clippy
- **Verify jobs**: test + docs lint + docs build + workflow lint
- **Mode-gated jobs**: coverage, dependency policy, MSRV
- **`release-plz.yml`**: `push(main)` release path, reusable CI preflight, `release-plz release`, `release-plz release-pr`, direct Pages handoff
- **`pages.yml`**: reusable/manual Pages build + deploy (mdBook + rustdoc)
- **`security.yml`**: nightly-only advisory scan + beta/nightly canary
- **Pre-commit**: markdownlint + dprint + file hygiene

Operational repo health tied to delivery:

- `.github/dependabot.yml` updates `cargo` and `github-actions` weekly
- `SECURITY.md` defines private vulnerability reporting and supported-version policy
- `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue templates, PR template, `CODEOWNERS`, `FUNDING.yml`, `CITATION.cff` are present
- `LICENSE-MIT` and `LICENSE-APACHE` are both checked in
- clean-checkout docs builds depend on tracked mdBook assets: `mdbook-admonish.css`, `mermaid.min.js`, `mermaid-init.js`

---

## Critical Gotchas [stable]

These are the facts most likely to cause subtle breakage if forgotten.

**Color**: BGR internally, not RGB. Red = `0x0000FF`.

**Units**: `1pt = 100 HwpUnit`, `1mm ≈ 283 HwpUnit`.

**HWPX shapes**: geometry uses `hc:` namespace (`<hc:startPt>`, `<hc:endPt>`), not `hp:`.

**Line element order**: geometry must come before sizing (`sz` / `pos` / `outMargin`).

**Equations**: HancomEQN script format, not MathML. No shape common block. Always inline (`treatAsChar="1"`, `flowWithText="1"`).

**Charts**: OOXML `<c:chartSpace>` format. Bar vs Column share `<c:barChart>` and differ by `barDir`. Chart XML parts are not listed in `content.hpf`.

**HWP5 field IDs**: real-world implementations diverge (`%dte`, `$dte`, `%dat`, `%smr`, `$smr`). Parser policy is alias normalization + unknown preservation.

**HWP5 table page break**: current controlled fixture truth is `0=None`, `1=Table`, `2=Cell`. Hancom UI wording and some reference prose do not line up cleanly with the saved enum truth.

**HWP5 BinData images**: `DocInfo/BinData compression=Default` entries require per-entry payload decompression before HWPX emission. Raw stream bytes are not always display-ready image bytes.

**HWP5 table fixture labels**: `table_09a_page_break_cell` is misnamed; saved truth is `TABLE` mode. Trust emitted companion HWPX, not the filename.

**HWP5 table parity**: `page_break`, `repeat_header`, `cell_margin`, `vertical_align`, table/cell `border_fill_id`, positive `cell.height`, and structured `table_cell_evidence` now round-trip through Core/HWPX/HWP5 for the current controlled fixtures. Remaining table presentation backlog is richer row/table sizing semantics and broader public-document border/fill fidelity.

**HWPX table omitted/default handling**: missing `repeatHeader` now defaults to `true`; missing `pageBreak` now defaults to `CELL`. Explicit unknown `pageBreak` values are invalid structure, not silently normalized.

**HWP5 unknown table page break**: unknown raw `page_break` values must surface as `ProjectionFallback` warning-first behavior. Silent normalization is forbidden.

**Reusable workflow semantics**: `workflow_call` makes `github.event_name == 'workflow_call'`, so mode must be passed explicitly.

**Pages handoff**: Pages deploy is called directly from `release-plz.yml`; it does not rely on tag-push fan-out anymore.

**Docs build**: `mdbook-admonish.css`, `mermaid.min.js`, and `mermaid-init.js` must stay tracked in the repo. Clean-checkout docs builds fail if they are ignored.

**Workspace lockfile policy**: `Cargo.lock` is still ignored in `.gitignore`, so workspace-wide CI commands must not assume `--locked`.

**`.docs` workflow rule**: `.docs/` is treated as local planning/research workspace in this repo workflow. Do not assume `git status` will surface `.docs` edits; operationally treat `.docs` updates as local memory/planning changes unless the user explicitly asks to promote them into tracked changes.

---

## Key Decisions [stable]

- **TDD**: edge cases first, normal cases last
- **YAGNI**: avoid speculative features
- **100% rustdoc**: public APIs are documented
- **Warning-first for unknowns**: unsupported or unknown source semantics must surface warnings or validation errors first
- **No fake support**: silent normalization of unknown values into arbitrary defaults is forbidden
- **Current dependency baseline**: quick-xml 0.39, zip 8.1, pulldown-cmark 0.13, schemars 1.2
- **Release automation**: `release-plz` owns release PRs and publish flow
- **MSRV policy**: stable minus 4 releases; source of truth is `Cargo.toml` `rust-version`
- **HWP5 execution rule**: if HWP5 needs semantics that Core/HWPX cannot represent or round-trip, implement the HWPX/Core side first and wire HWP5 after the shared model is proven

## Documentation Map [stable]

| Path                        | Purpose                                               |
| --------------------------- | ----------------------------------------------------- |
| `AGENTS.md`                 | Root agent context, current phase, CI/CD, repo health |
| `crates/*/AGENTS.md`        | Crate-local responsibilities and gotchas              |
| `MEMORY.md`                 | Centralized project snapshot                          |
| `README.md`                 | User-facing overview and project status               |
| `CONTRIBUTING.md`           | Contributor workflow and MSRV policy                  |
| `SECURITY.md`               | Vulnerability reporting and support window            |
| `.docs/planning/ROADMAP.md` | Roadmap SSoT                                          |
| `.docs/architecture/`       | Type system, crate roles, TDD guidance                |
| `.docs/`                    | Internal planning / research / references             |

## Dev Commands [stable]

```bash
make ci          # alias of ci-fast
make ci-fast     # fmt + clippy + test + deny + lint-md
make ci-full     # ci + coverage + MSRV
make test        # cargo nextest run --workspace --all-features
make clippy      # cargo clippy --workspace --all-targets --all-features -- -D warnings
make cov         # cargo llvm-cov nextest --workspace --all-features --fail-under-lines 90 --html
make msrv        # cargo +1.88 check --workspace --all-features
bacon            # watch mode
```
