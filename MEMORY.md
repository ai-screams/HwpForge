# MEMORY.md -- HwpForge Project Knowledge Base

> Last Updated: 2026-03-07
> Sections marked [stable] rarely change; [volatile] should be re-verified after major feature or workflow changes.

## Identity [stable]

| Field              | Value                                             | Field                | Value               |
| ------------------ | ------------------------------------------------- | -------------------- | ------------------- |
| Name               | HwpForge                                          | License              | MIT OR Apache-2.0   |
| Purpose            | Programmatic control of Korean HWP/HWPX documents | Language             | Rust (edition 2021) |
| Version            | 0.1.0                                             | MSRV / Dev Toolchain | 1.88 / 1.93         |
| Workspace          | 9 Cargo packages                                  | Tracked Rust LOC     | 61,577              |
| Tests              | 1,510 nextest / 1,702 cargo test-discovered       | Coverage Gate        | 90%+ in CI          |
| Tracked Rust Files | 93                                                | Unsafe               | Forbidden           |
| Design             | Clean Room (ideas only, no code copy)             | CI workflows         | 4                   |

## Architecture [stable]

```
Foundation (HwpUnit, Color, IDs)
  -> Core (document DOM, style references only)
  -> Blueprint (YAML styles and style registry)
  -> Smithy (HWPX / Markdown compilers, HWP5 planned)
  -> Bindings (CLI / Python planned, MCP planned)
```

Key principle: **Structure** (Core) and **Style** (Blueprint) stay separate, like HTML + CSS.

---

## Workspace Packages [volatile]

| Package                 | Role                                         | Status         |
| ----------------------- | -------------------------------------------- | -------------- |
| `hwpforge`              | Umbrella facade crate (`hwpx`, `md`, `full`) | ACTIVE         |
| `hwpforge-foundation`   | Primitives, units, colors, branded indices   | COMPLETE       |
| `hwpforge-core`         | Format-agnostic document model               | COMPLETE       |
| `hwpforge-blueprint`    | YAML template/style system                   | COMPLETE       |
| `hwpforge-smithy-hwpx`  | HWPX decoder/encoder                         | COMPLETE       |
| `hwpforge-smithy-md`    | Markdown bridge                              | COMPLETE       |
| `hwpforge-smithy-hwp5`  | HWP5 reader                                  | STUB / v2.0    |
| `hwpforge-bindings-cli` | CLI entrypoint                               | STUB / Phase 6 |
| `hwpforge-bindings-py`  | Python bindings                              | STUB / Phase 6 |

Write-path completion status:

- Phase 0-5 complete
- Phase 4.5 Wave 1-6 complete
- Phase 5.5 complete
- Wave 7-14 complete

---

## Roadmap Snapshot [volatile]

|     Phase | Description                        | Status          |
| --------: | ---------------------------------- | --------------- |
|         0 | Foundation                         | DONE            |
|         1 | Core DOM                           | DONE            |
|         2 | Blueprint styles                   | DONE            |
|       3-4 | Smithy-HWPX read/write             | DONE            |
|       4.5 | Extended write features (Wave 1-6) | DONE            |
|         5 | Smithy-MD                          | DONE            |
|       5.5 | Zero-config write API              | DONE            |
| Wave 7-14 | Remaining HWPX write surface       | DONE            |
|         6 | CLI bindings                       | NEXT            |
|         7 | MCP server                         | PLANNED         |
|         8 | Testing + release hardening        | PLANNED         |
|        10 | HWP5 reader                        | DEFERRED (v2.0) |

Planning sources:

- `.docs/planning/ROADMAP.md`
- `.docs/planning/PHASE6_CLI_DETAILED.md`
- `.docs/planning/PHASE7_MCP_SERVER_DETAILED.md`

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

**Reusable workflow semantics**: `workflow_call` makes `github.event_name == 'workflow_call'`, so mode must be passed explicitly.

**Pages handoff**: Pages deploy is called directly from `release-plz.yml`; it does not rely on tag-push fan-out anymore.

**Docs build**: `mdbook-admonish.css`, `mermaid.min.js`, and `mermaid-init.js` must stay tracked in the repo. Clean-checkout docs builds fail if they are ignored.

**Workspace lockfile policy**: `Cargo.lock` is still ignored in `.gitignore`, so workspace-wide CI commands must not assume `--locked`.

---

## Key Decisions [stable]

- **TDD**: edge cases first, normal cases last
- **YAGNI**: avoid speculative features
- **100% rustdoc**: public APIs are documented
- **Current dependency baseline**: quick-xml 0.39, zip 8.1, pulldown-cmark 0.13, schemars 1.2
- **Release automation**: `release-plz` owns release PRs and publish flow
- **MSRV policy**: stable minus 4 releases; source of truth is `Cargo.toml` `rust-version`

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
