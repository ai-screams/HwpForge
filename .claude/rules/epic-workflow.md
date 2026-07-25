# 정식 에픽 워크플로우

E3(표 격자 주소)·E4(문단 구조 편집)·E5(outline/read/diff)·E6(템플릿 스탬핑)으로 검증된 프로세스.
새 에픽/슬라이스는 아래 게이트를 **순서대로** 통과한다. 단계별 상세 규칙이 CLAUDE.md 의 다른 절에
있으면 그쪽이 canonical — 이 파일은 순서·게이트·금지사항만 정의한다.

## 0. 사전 확인 (ground truth)

- root `AGENTS.md` → `crates/AGENTS.md` → 대상 크레이트 로컬 `AGENTS.md` 순으로 읽는다.
- 로드맵/브랜치 prose 를 믿지 말고 **코드·매니페스트·git** 에서 현재 상태를 확인한다.
- workspace grep 으로 **이미 구현된 레이어**를 확인한다 (중복 구현 방지).
- HWP5 가 새 semantic 을 드러내면 공유 모델(Core)이 carry 가능한지부터 확인한다 (shared-model first).
- public API/semver 파괴 가능성이 보이면 **즉시 멈추고 사전 승인**을 받는다.

## 1. 연구·설계 재검토 (`.docs/planning/`)

- `.docs/planning/YYYY-MM-DD-<epic>.md` 에 설계를 작성한다 (내부 문서 — **git 커밋 금지**).
- 설계 결정마다 **실측 근거**를 단다: 코퍼스 측정·네이티브 한컴 fixture 대조·바이트 검증.
  가정으로 진행하지 않는다 (예: E4 거부 규칙은 정부서식 corpus 과잉거부 0.00% 실측으로 확정).
- 네이티브 fixture 가 필요하면 **사용자에게 제작을 요청**한다 (한컴에서 만들어줄 수 있음).

## 2. Codex 적대 리뷰 (설계 단계)

- 설계안을 Codex 와 토론해 구멍을 찾는다 (`codex:codex-rescue`).
- 반영/기각 결과를 계획 문서에 기록한다 (실측이 Codex 원안과 다르면 실측이 이긴다).

## 3. 확정 계획 보고 → 사용자 승인 (필수 게이트)

- **승인 전 구현 착수 금지.** 확정 계획(범위·웨이브 분해·수용 기준·semver 영향·리스크)을
  보고하고 승인을 받는다.

## 4. TDD 웨이브 구현

- W1..Wn 웨이브 단위로 진행: edge-first TDD → atomic conventional commits (breaking 은 `type!:`).
- 커밋/푸시·훅·nextest·clippy 함정은 CLAUDE.md **"Tooling Gotchas"** 절이 canonical
  (commit/push 는 run_in_background + 파일 리다이렉트, 파이프 금지 등).
- 구현 중 설계와 다른 실측이 나오면 그 자리에서 계획 문서에 수정 근거를 기록한다.

## 5. 시각 게이트 (사용자 판정)

- 실제 산출물을 `examples/hwp5_review/_verify/` 에 생성하고 `open <절대경로>` 로 제시한다.
- 레이아웃에 닿는 편집(재인코드 경로 포함)은 **PDF 대조**(쪽수·줄바꿈)까지 한다 —
  admission 은 Core 비교라 wire 캐시(linesegarray 등) 소실을 못 본다.
- **사용자 판정 없이 통과 처리 금지.**

## 6. 독립 리뷰 + 상환

- 구현 컨텍스트와 **분리된 lane** 에서 코드리뷰를 받는다 (같은 컨텍스트 자기승인 금지).
- Critical/High = 즉시 수정. Medium/Low = 상환하거나 **백로그로 명시 문서화** (무음 드롭 금지).

## 7. CI → PR → merge queue

- push 전 `make ci` (플래그 일치: `--all-targets`·`--all-features`·fmt `--all`).
- coverage 게이트 ≥90% — **linux 가 macOS 보다 ~0.02–0.06% 낮게** 나오므로 마진을 확보한다.
- PR 제목·본문은 **한글**. 머지는 GraphQL `enqueuePullRequest` 로만 (CLAUDE.md "Releasing" 절 canonical).

## 8. 릴리스 (release-plz 소유)

- Release PR 머지 후 release-plz·npm-publish workflow success + crates.io sparse index·
  GitHub Release·npm 레지스트리 버전을 **실측 검증**한다 (추측 보고 금지).
- 버전/태그/publish 수동 조작 금지 (CLAUDE.md "Releasing" 절 canonical).

## 9. 기록

- memory `MEMORY.md` 체크포인트 + CLAUDE.md **Current Status** 스냅샷 갱신 (릴리스 후 docs PR).
- 에픽 상세 이력은 `.docs/planning/` 계획 문서에 남긴다 (CLAUDE.md 에 wave-by-wave 재축적 금지).
