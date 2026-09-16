# 프로젝트 상태 스냅샷

> 이 파일은 CLAUDE.md 로딩 맵에서 필요 시 로드된다 (자동 로드 아님).

**Current Status** (snapshot — 2026-08-28):

- HWPX codec: read/write shipped · Markdown bridge: read/write shipped
- HWP5 → HWPX converter path: active, style/layout fidelity line in progress
- CLI bindings: shipped · MCP bindings: shipped · Python bindings: stub
- Shared `tab` / `ordered·bullet·outline` / checkable-bullet semantics wired through core → blueprint → smithy. HWP5 checkable carries all three DP#8 (`design-patterns.md`) truth locations (`bullet.checkedChar`, `bullet.paraHead.checkable`, `paraPr.checked`).
- Phase 12 HWP5→HWPX carry series (GSO shapes/equation/memo/dutmal/compose/indexmark/click-here·auto fields/cross-ref instId/document metadata/outline 1–10) `main` 머지 완료.
- **E6 IR 와이어-누출 상환 완료** (`0.9.0`): Summery rename(A) · BookmarkName collapse(B) · raw wire 필드 제거(C) · `inst_id`/`SystemId`→공유 `ObjectId`(M2). **H1(display_text)=Won't-do** (ADR-009 §CLOSURE, memory `e6-wire-leak-status-h1-wontdo.md`).
- **0.10.0 릴리스** (2026-07-02): colLine(다단 구분선) HWPX+HWP5 carry(breaking) + rmcp 2.0 보안(GHSA-89vp-x53w-74fx)·quick-xml 0.41 + 문서 정합.
- **AI 편집(에이전트 편집) 전 에픽 배포 완료** (2026-07-12~24, `0.11.0`→`0.11.6`): E1 누름틀 채우기(`0.11.0`, `feat!:` `display_text` 의미 변경) · E2 `fill` 델타 API·`fields`(`0.11.1`) · E6 템플릿 스탬핑 W1(`0.11.2`) · E3 표 격자 주소+`set-cell`(`0.11.3`) · E6 W2 클래스-B 셀 스탬핑+`layout_carry` linesegarray 보존(`0.11.4`) · E5 outline/read/diff 읽기 3표면(`0.11.5`) · E4 문단 구조 편집 `insert-para`/`delete-para`(`0.11.6`, preserve-first 바이트 스플라이스 + reverse-delta self-verify). 설계/로드맵 = `.docs/planning/2026-07-10-agent-editing-architecture.md` (남은 후속: E4b 표 행 편집 · `<hp:t>` 줄 경계 분할 보존 등 backlog).
- **PDF Export 에픽 배포** (2026-07-26~08-12, `0.12.0`→`0.13.1`): smithy-pdf 신설 — 조판 캐시 **재생**(계산 금지) 렌더: 표·머리말/꼬리말·쪽번호·폰트 파이프라인(face 축·fsType)·CLI `to-pdf`(스니핑·3채널 경고 DTO). 에픽 canonical = `.docs/planning/2026-07-26-pdf-export-epic.md`.
- **이미지/글상자 렌더 에픽 W1b~W5 배포** (2026-08-14~25, `0.14.0`→**`0.16.0`**): W1b 좌표 ledger(가시 textpos 통일, `0.15.0`) · W2 인라인 이미지(`0.15.1`) · W3 표 셀 이미지+축약점 earliest-preimage(`0.15.2`) · W4 글상자 렌더+**ObjectPlacement 공용화(breaking — 도형 11종 hp:pos 캐리)** · W5 글상자 내부 인라인+body 앵커 렌더+HWP5 앵커 비트 byte-ground → **`0.16.0`** (트리거 3막 사고 — `RELEASING.md` §8 필수 조건 2건 참조). 에픽 canonical = `.docs/planning/2026-08-13-image-textbox-epic.md` (잔여: sub-line-height 인라인 이미지 · W6 마감 · CI 다이어트 제안 `.docs/planning/2026-08-25-ci-diet-proposal.md`).
- **각주/미주 MD 브리지 에픽 완주** (2026-08-27~28, **`0.16.4`**): `[^N]`/`[^eN]` 양방향 왕복 (다문단·명명 라벨·표 셀·인라인 서식) + autoNum 번호 머리(**대칭 쌍 계약** — WG#34) + 편집 표면 fail-closed. 한컴 native fixture F1~F7 게이트 · 시각 게이트 3회 · 적대 리뷰 8라운드 수렴. 에픽 canonical = `.docs/planning/2026-08-27-footnote-endnote-md-bridge.md` (HITL 4건 백로그: ON_SECTION 정책·note format 승격·validate 값 범위·편집기 warning API).

> **이 섹션은 짧은 상태 스냅샷으로만 유지한다 (wave-by-wave 이력을 여기 다시 쌓지 말 것).**
> Wave별 상세 이력 + breaking change: umbrella **`crates/hwpforge/CHANGELOG.md`**(release-plz, canonical) + 크레이트별 `crates/*/CHANGELOG.md` + GitHub Releases (루트 `CHANGELOG.md` 는 0.9.0 이후 정지 — 참조하지 말 것) 와 memory `MEMORY.md` / `phase11_wave_history.md`.
> Enum/wire 레이아웃 표 (번호·쪽번호·이미지채우기·대각선 등): **`crates/hwpforge-smithy-hwp5/HWP5_WIRE_SPEC.md`** (특히 §22).

**Still-deferred (Windows 한컴 fixture 대기)**:

- **non-chart OLE passthrough** — 전 구간 구현됐으나 standalone `<hp:ole>` 가 macOS 한컴 crash, macOS는 생성 자체 불가 → `git stash` 보존 (memory `non-chart-ole-deferred.md`)
- **masterPage carry** (Wave 5 gap C, task #33) · **쪽 테두리/배경 hatch _페이지_ 경로** (char/table hatch 는 byte-verified 완료, 공유 코드 — macOS [쪽] 메뉴에 항목 없음)
- **양식컨트롤(form controls)** — 완전 무음 드롭, `b"form"` + `HWPTAG_FORM_OBJECT(0x5B)` 미구현 (memory `form-controls-deferred.md`)
- **가운데 밑줄** (macOS 한글 밑줄 위치에 "가운데" 옵션 없음) · 한컴-authored multi-run span 디코딩 (편집 prerequisite, task #96)

**Known lossy (Core breaking 필요, 후속 슬라이스)**: 글자 그림자 색·위치, 스크립트별 자간/장평, 한영 자동 간격, 문단 세로정렬·테두리 오프셋 등 (P2 — 경고는 나감) + enum 천장 (P3 — UnderlineShape/StrikeoutShape/EmphasisType 등 raw 초과분). 상세 backlog: `.docs/planning/BACKLOG_SMITHY_HWPX.md` + 분석 `.docs/audit/2026-06-17_hwp5_hwpx_option_gaps.md`.

**Workspace Facts** (code-grounded — 카운트는 drift하니 인용 전 확인):

- Cargo packages `12` (smithy-pdf 포함) · crates.io published `0.16.4` (각주/미주 MD 브리지, 2026-08-28) · MSRV `1.88` · Dev toolchain Rust `1.93`
- `crates/` 추적 src 파일 ~`228` · nextest(make ci) ~`3,476` passed + `14` skipped · `examples/` 산출물 `68`+ (미추적 `examples/hwp5_review/` 리뷰 영역 별도 — gitignore 아님) · GitHub workflows `5`

---

---

## Current Engineering State

- 릴리스/기능 상태의 canonical = 상단 **Current Status** 스냅샷 (여기 중복 서술 금지).
- Table integration gates are concentrated in `crates/hwpforge-bindings-cli/tests/cli_integration.rs`.
- Stress or real-world table fixtures are not the same thing as committed regression gates.
- colLine (다단 구분선) HWPX + HWP5→HWPX legs shipped in `0.10.0` (PR #91, 2026-07-02).
- **Nightly › Fuzz Build 복구됨** (PR #101, 2026-07-19 — 2026-06-29부터 실패했었음): 원인 2겹 = ① prebuilt cargo-fuzz 가 musl 을 기본 타깃으로 골라 ASAN 과 충돌 → `security.yml` 에 `--target x86_64-unknown-linux-gnu` 명시 ② fuzz 타깃 bit-rot (`hwp5_to_hwpx_bytes` 가 convert 크레이트로 이사). fuzz/ 는 standalone 워크스페이스라 메인 CI 가 컴파일을 안 잡음 — API 이동 시 fuzz 타깃도 함께 갱신할 것.
- Always confirm `main` state from code + manifests + git; do not trust stale branch prose.

---
