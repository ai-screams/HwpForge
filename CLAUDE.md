# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository. Contributor workflow (PR, release) lives in `CONTRIBUTING.md` and `RELEASING.md`.

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

## Architecture (Forge Metaphor)

The codebase follows a **blacksmith workshop** metaphor with clear separation of concerns (canonical names: `branding/BRANDING.md` "대장간 이름 체계"):

```
Foundation = Ingot (primitives)
  → Core = Anvil (pure document structure, no style definitions; the hub every format passes through)
  → Blueprint = Blueprint (YAML style templates, centralized like Figma Design Tokens)
  → Smithy = Hearth (format-specific compilers: HWPX, HWP5, Markdown, PDF)
  → Bindings: CLI = Hammer 🔨 · MCP = Vise 🗜️ · Python = Tongs 🐍
```

**Key Principle**: **Structure and Style are separate** (like HTML + CSS).

- Core contains document structure with style **references** (IDs only)
- Blueprint contains style **definitions** (fonts, sizes, colors)
- Smithy compilers fuse Core + Blueprint → final format

This enables:

- One YAML template applied to multiple documents
- Format-agnostic document manipulation
- Easy addition of new formats — smithy-pdf is a real, shipped example (0.12.1+); smithy-odt remains a hypothetical one

---

## Development Commands

```bash
make ci               # push 전 필수 — ci-fast 별칭 (fixture-policy·fmt·clippy·test·deny·lint-md, CI 플래그 일치)
make ci-full          # 위 + coverage + MSRV (릴리스·큰 변경 전)
make py-all           # Python 바인딩 (lint·타입·Rust/Python 테스트·coverage)
make site-check       # 문서 사이트 빌드·후처리 (CI Docs Build 와 같은 경로, mdBook 0.4 필요)
mdbook build          # 문서 변경 시 — mdBook 0.4 라인 필수 (`make install-tools`)
cargo nextest run -p <crate>          # 크레이트 한정 (전 워크스페이스 cold 15분+ 금지)
cargo clippy -p <crate> --all-targets -- -D warnings   # 커밋 전 touched 크레이트 사전 점검
bacon / bacon test    # watch 모드
```

`make ci` 는 CI 전체가 아니라 빠른 여섯 레인(fixture-policy·fmt·clippy·test·deny·lint-md)이다 — CI 는 여기에 Coverage·MSRV·Docs Build·Python(release 전용 테스트 포함)·Workflow Lint 를 더 돌린다.

---

## Working Principles

- **Warning-first for unknowns**: if source truth is missing or a value is unsupported, emit a warning or validation signal first.
- **No fake support**: do not silently normalize unknown semantics into arbitrary defaults just to keep output green.
- **Unhandled enum ≠ bug**: an unmatched enum arm is a real gap only if that value actually exists in the reference enum (hwpxlib/libhwp) or in a document saved by Hancom Office. Verify existence first — otherwise `_ => default` is correct and a guessed mapping is fake support. (See `HWP5_WIRE_SPEC.md §22`; 번호 code 11 / 이미지 채우기 모드 = real bugs, 가나다 · 대각선 1/4/5 = false positives.)
- **Shared-model first**: if HWP5 discovers a semantic that Core/HWPX cannot carry, extend the shared representation first and wire HWP5 after.
- **Semver-first for public API**: if a design touches public structs, enums, or externally constructible types, surface the breakage before implementation and get approval first.
- **편집 표면은 의미 손상 fail-closed**: stamper/cell-edit 등 preserve-first 편집기는 인코드가 의미 경고(`NoteHeadSkipped` 등)를 내면 기존 오류 타입으로 거부한다 — admission(Core 비교)은 wire 손상을 못 보므로 무경고 성공 반환 금지.

구현 중 상시 규칙: **TDD edge-first** · **atomic conventional commits** (breaking 은 `type(scope)!:`) · **100% rustdoc** (`#![deny(missing_docs)]`) · **zero clippy warnings**.

## Repository Rules

1. **Color 는 BGR** — `Color::from_rgb()` 만 사용 (`from_raw(0xFF0000)` 은 파랑).
2. **버전/태그/publish 수동 조작 금지** — release-plz 소유. main 머지는 merge queue(squash)로만.
3. **`Cargo.lock`·`uv.lock` 은 추적하지 않는다** — `.gitignore` 대상이라 `git add` 가 거부한다. CI 는 최신 의존성으로 푼다.
4. **테스트 실행 중 소스 편집 금지** (rebuild 유발) · nextest 필터는 **substring** (`'a|b'` 무효).
5. PR 제목은 **English conventional**(squash 라 PR 제목 = main 커밋 = release-plz 입력, breaking 은 `type!:`).
6. **다른 앱이 저장한 문서는 커밋하지 않는다** — `.hwp`·`.pdf`·HwpForge 인코더 서명이 없는 `.hwpx` 는 `scripts/check_fixture_policy.py` 가 막는다. 시험 입력은 레포 안 예제·코드로 다시 만들 수 있어야 한다.
