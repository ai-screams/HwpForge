# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **구조 원칙**: 이 파일은 매 세션 로드되는 **얇은 인덱스**다 — 상세는 아래 **로딩 맵**의
> 파일을 필요할 때 읽는다. 여기에 이력·상세를 다시 쌓지 말 것 (추가는 로딩 맵 대상 파일에).

---

## Project Overview

HwpForge is a Rust library for programmatic control of Korean HWP/HWPX document formats, designed with LLM-first principles. The goal is to enable AI agents (like Claude Code) to generate Korean government proposal documents using natural language + Markdown + YAML style templates.

현재 crates.io published = **0.16.4** (2026-08-28, 각주/미주 MD 브리지). 상태 스냅샷·에픽 이력 = `.claude/guides/status.md`.

---

## 로딩 맵 (필요할 때 읽는다 — 해당 작업 전 필독)

| 상황 (트리거)                          | 읽을 파일                                                                                                |
| -------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| 에픽/슬라이스 시작 · 현재 상태 인용 전 | `.claude/guides/status.md` (스냅샷·deferred·lossy·Workspace Facts)                                       |
| **커밋·푸시·테스트 실행 전**           | `.claude/guides/tooling.md` (훅·nextest·lock·디스크 함정)                                                |
| 새 타입/API 설계 · 테스트 작성         | `.claude/guides/design-patterns.md` (패턴 11종·크레이트 그래프·TDD·레퍼런스)                             |
| **HWPX/HWP5 wire 구현·디버깅 전**      | `.claude/guides/wire-gotchas.md` (WG#1~37; 코드 예제판 = `.docs/references/gotchas.md` (RG#, 번호 독립)) |
| 릴리스·머지 큐·publish 검증            | `RELEASING.md` (canonical — §8 운영 함정 포함)                                                           |
| 에픽 정식 절차                         | `.claude/rules/epic-workflow.md` (자동 로드)                                                             |
| 에이전트 경계/크레이트 규칙            | root `AGENTS.md` → `crates/AGENTS.md` → 크레이트 로컬                                                    |
| 내부 문서·계획·참조 자료 위치          | `.docs/README.md` (git 밖, 인덱스)                                                                       |

---

## Architecture (Forge Metaphor)

The codebase follows a **blacksmith workshop** metaphor with clear separation of concerns:

```
Foundation (🔩 primitives)
  → Core (🔨 pure document structure, no style definitions)
  → Blueprint (📐 YAML style templates, centralized like Figma Design Tokens)
  → Smithy (🔥 format-specific compilers: HWPX, HWP5, Markdown, PDF)
  → Bindings (🐍⚒️🤖 Python/CLI/MCP interfaces)
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
make ci-fast          # 빠른 로컬 검증
make ci               # push 전 필수 (CI 플래그 일치: --all-targets --all-features)
cargo nextest run -p <crate>          # 크레이트 한정 (전 워크스페이스 cold 15분+ 금지)
cargo clippy -p <crate> --all-targets -- -D warnings   # 커밋 전 touched 크레이트 사전 점검
bacon / bacon test    # watch 모드
```

상세 (coverage·doc·훅 함정 전체) = `.claude/guides/tooling.md`.

---

### Working Principles

- **Warning-first for unknowns**: if source truth is missing or a value is unsupported, emit a warning or validation signal first.
- **No fake support**: do not silently normalize unknown semantics into arbitrary defaults just to keep output green.
- **Unhandled enum ≠ bug**: an unmatched enum arm is a real gap only if that value actually exists in the reference enum (hwpxlib/libhwp) or a native fixture. Verify existence first — otherwise `_ => default` is correct and a guessed mapping is fake support. (See `HWP5_WIRE_SPEC.md §22`; 번호 code 11 / 이미지 채우기 모드 = real bugs, 가나다 · 대각선 1/4/5 = false positives.)
- **Shared-model first**: if HWP5 discovers a semantic that Core/HWPX cannot carry, extend the shared representation first and wire HWP5 after.
- **Semver-first for public API**: if a design touches public structs, enums, or externally constructible types, surface the breakage before implementation and get approval first.
- **편집 표면은 의미 손상 fail-closed**: stamper/cell-edit 등 preserve-first 편집기는 인코드가 의미 경고(`NoteHeadSkipped` 등)를 내면 기존 오류 타입으로 거부한다 — admission(Core 비교)은 wire 손상을 못 보므로 무경고 성공 반환 금지.

구현 중 상시 규칙: **TDD edge-first** · **atomic conventional commits** (breaking 은 `type(scope)!:`) · **100% rustdoc** (`#![deny(missing_docs)]`) · **zero clippy warnings**.

---

## 치명 규칙 Top (전 작업 상시 — 로딩 맵과 무관하게 항상 적용)

1. **Color 는 BGR** — `Color::from_rgb()` 만 사용 (`from_raw(0xFF0000)` 은 파랑).
2. **`.docs` 는 git 에 절대 커밋/푸시 금지** (내부 문서).
3. **commit/push 는 `run_in_background` + 파일 리다이렉트** — 파이프(`| tail`) 연결 금지 (훅 출력이 push 를 죽임 · 실패 라인 유실). 성공 판정은 `git ls-remote` 실측.
4. **버전/태그/publish 수동 조작 금지** — release-plz 소유. 머지는 GraphQL `enqueuePullRequest` 로만 (`gh pr merge` 거부됨).
5. **테스트 실행 중 소스 편집 금지** (rebuild 유발) · nextest 필터는 **substring** (`'a|b'` 무효).
6. `examples/README.md` 미커밋 변경 = 사용자 낙서 — 보존 말고 **항상 원복**, 커밋 절대 금지.
7. PR/이슈 생성 시 **assignee = 나** (`gh --assignee @me`) · PR 제목·본문 **한글** · PR 생성/큐 등록은 **사용자 명시 승인 후에만**.
8. stale `.git/index.lock`(0바이트·git 프로세스 없음 확인) 은 `rm -f` — 반복 사고.

---

> **정식 에픽 워크플로우 = `.claude/rules/epic-workflow.md`** (자동 로드 — E3~E6·E4 로 검증된 프로세스, 그쪽이 canonical).
> 요약: 사전 확인(ground truth) → 연구·설계(`.docs/planning/`, 실측 근거) → 적대 리뷰 → **확정 계획 보고·사용자 승인** → TDD 웨이브 → 시각 게이트(사용자 판정·PDF 대조) → 독립 리뷰 상환 → CI·merge queue → release-plz 릴리스 실측 검증 → 기록.
