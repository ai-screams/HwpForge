# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- styled Markdown 출력에서 본문 문단과 표 칸의 글자가 줄 머리에서 `- `·`1. `·`: `·`===`·표 구분 행처럼 Markdown 구조로 다시 읽히지 않도록 그 표지 앞에 `\` 를 붙인다. 목록·제목·각주·캡션과 lossy 출력은 이전과 같다(정의 목록이 생기던 블록만 아래처럼 바뀐다).
- 문단 안 표 바로 뒤의 글이 표에 흡수되던 문제를 고쳤다. 표 뒤에 빈 줄을 넣어 뒤 글이 새 문단으로 남고, GFM 표 행에서 버려지던 글자도 보존된다.
- 위 규칙으로 막을 수 없는 드문 본문 문단은 서식 없는 글로 바꾸고 `MarkupWrittenAsText` 경고를 남긴다. 상호참조·필드·알 수 없는 컨트롤은 대체 글에서도 정상 출력과 같은 표시값을 쓴다.
- 출력에 정의 목록이 남지 않아, 그 때문에 from-md 가 실패하던 경우가 없어진다.

## [0.17.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.17.0...hwpforge-smithy-md-v0.17.1) - 2026-10-24

### Changed

- 인코더의 자체 중첩 깊이 계산을 지우고, 검증된 문서가 깊이 32단계 이하라는 Core 의 보장에 맡긴다. 검증된 문서의 출력은 같다 (#267)

### Fixed

- 이미지 자산이 없는 그림(빈 경로)은 styled·lossy 출력에서 빈 링크를 내지 않고 캡션만 남기며, 이미지 자산 계획에서도 뺀다 (#266)

## [0.17.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.11...hwpforge-smithy-md-v0.17.0) - 2026-10-23

### Changed (BREAKING)

- `MdOutput` 에 `warnings: Vec<MdWarning>` 가 생겨 `#[non_exhaustive]` 가 되었다. `MdWarning::ContainerTooDeep` 이 생겼다.

### Changed

- 읽기용 출력의 이스케이프를 하나의 타입 있는 계층으로 모으고, CommonMark 가 마크업으로 읽을 글자만 이스케이프한다. 링크는 `http://`, `https://`, `mailto:` 만 링크로 쓴다. 표가 8겹을 넘으면 `(중첩 표 생략)` 으로 쓰고 경고한다. 컨테이너가 32겹을 넘으면 읽기용 출력은 오류로 거부하고, 스타일 출력은 그 아래를 생략하며 경고한다.
- 글상자·각주·도형 안의 표와 그림, 묶음, 표·그림 캡션, 한컴이 저장한 필드의 표시값이 출력에 나온다.
- 무손실 Markdown 이 묶음과 그 그림, 도형 여덟 가지, 본문 그림의 배치와 캡션, 개체 틀, 표 캡션 속성·셀 구역·고정 셀 크기·셀 줄 바꿈, 그림 하이퍼링크를 왕복한다. 이전 판은 이 요소들을 읽지 못한다.

### Fixed

- 중첩 표 출력이 층마다 두 배로 불어나던 문제, 코드 펜스 검사의 이차 비용, 묶음 안 그림 링크와 캡션 속 그림 바이트가 빠지던 문제.

## [0.16.10](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.9...hwpforge-smithy-md-v0.16.10) - 2026-10-04

### Documentation

- correct the statements a fact check found wrong

- *(readme)* match the READMEs to the code and drop volatile counts


### Fixed

- *(md)* `to-md` 의 수식 변환이 깊게 중첩된 수식에서 스택을 넘쳐 프로세스가 죽던 문제를 고친다. 중첩 32단에서 멈추고, 그보다 깊은 수식은 `[수식 변환 생략: 중첩 깊이 초과] ` 뒤에 공백을 접은 원문을 그 자리의 일반 텍스트처럼 이스케이프해 쓴다. 실제 문서 수식(측정 코퍼스 최대 3단)의 출력은 그대로다


## [0.16.8](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.7...hwpforge-smithy-md-v0.16.8) - 2026-10-01

### Fixed

- *(md)* 무손실 Markdown 을 다시 읽을 때 `&amp;`·`&lt;`·`&gt;`·`&quot;`·`&#39;` 같은 참조를 글자로 풀어 읽는다. 이전에는 `A & B` 가 `A  B` 로 돌아왔다. 모르는 엔티티나 잘못된 숫자 참조는 `LosslessParse` 오류 (#197)


## [0.16.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.6...hwpforge-smithy-md-v0.16.7) - 2026-10-01

### Documentation

- *(readme)* link the docs site from the root and MCP crate READMEs


### Fixed

- *(hwpx)* quick-xml 0.42 로 올린다. lossless Markdown 디코더를 0.42 의 문자열 API 로 옮긴다


## [0.16.6](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.5...hwpforge-smithy-md-v0.16.6) - 2026-09-22

### Added

- *(bindings-cli)* `validate` 명령 신설 — MCP·Python 에만 있던 검증 연산을 CLI 에도


### Documentation

- PDF 렌더의 전제를 "한컴 저장본" 이 아니라 "조판 캐시" 로 적는다

- README 퀵스타트와 Rust 예시가 적힌 그대로 실행되게 한다

- *(python)* Python 바인딩 0.16.5 첫 wheel 배포를 문서에 반영


## [0.16.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.4...hwpforge-smithy-md-v0.16.5) - 2026-09-17

### Added

- *(smithy-md)* AssetOutcome 을 kind 내부 태그 snake_case 로 직렬화

- *(smithy-md)* 이미지 자산 해석을 계획·파일 읽기·임베드 3단계 assets 계약으로 분리


### Documentation

- 브랜치 전체 리뷰 반영 — 릴리스 이력 포인터·architecture mermaid·MSRV 예외 정정

- 공개 문서를 0.16.4 실측에 맞게 정정 — CLI 22·MCP 19·smithy-pdf·설치 명령·정책 문서


### Fixed

- *(smithy-md)* finish_assets 가 호출자의 계획과 재수집 계획을 전건 대조


## [0.16.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.3...hwpforge-smithy-md-v0.16.4) - 2026-08-28

### Added

- *(smithy-hwpx)* 각주/미주 본문 autoNum 번호 머리 방출 (W3.5)

- *(smithy-md)* 각주 다문단 방출 + 행 시작 escape — 왕복 폐곡선 (W2)

- *(smithy-md)* 각주·미주 MD 디코딩 — [^N]/[^eN] 왕복 (W1)

- *(smithy-md)* 인라인 서식 스택 — 굵게/기울임/취소선 왕복 (W0)


### Fixed

- *(smithy-hwpx)* 각주 번호 머리 주입의 리뷰 지적 7건 상환


## [0.16.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.16.2...hwpforge-smithy-md-v0.16.3) - 2026-08-26

### Fixed

- *(smithy-md)* 임베드 리뷰 상환 — 단독 문단 보존·bare 경로 base·BMP 구조검사

- *(smithy-md)* md→hwpx 이미지 참조를 BinData 로 실제 임베드


## [0.14.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.13.1...hwpforge-smithy-md-v0.14.0) - 2026-08-13

### Added

- pghd 감추기 carry + 명시적 쪽나눔 렌더 존중 (W3)

- nwno 새 번호 지정 carry — HWP5→Core→HWPX→PDF 전 구간 (W2)


## [0.11.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.11.4...hwpforge-smithy-md-v0.11.5) - 2026-07-23

### Changed

- *(md)* styled 인코더를 core 공유 분류기로 재기저


## [0.11.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.11.2...hwpforge-smithy-md-v0.11.3) - 2026-07-21

### Added

- lossy MD 병합셀 평탄화 경고 + convert 병합 격자 회귀 잠금 (E3 Wave 4)


## [0.11.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.10.0...hwpforge-smithy-md-v0.11.0) - 2026-07-12

### Added

- *(hwpx)* 누름틀 본문을 preserve-first 텍스트 슬롯으로 등록


## [0.8.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.7.0...hwpforge-smithy-md-v0.8.0) - 2026-06-27

### Added

- *(core)* **BREAKING** shape text vertical alignment (ellipse/polygon/textbox)


### Documentation

- sync README/mdbook/CLAUDE for hwpforge-convert (E5) + refresh metrics


## [0.7.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.6.0...hwpforge-smithy-md-v0.7.0) - 2026-06-19

### Added

- *(core)* **BREAKING** Wave 12p Step 2 — Core breaking: Image/Table/Equation 에 inst_id (cross-ref target carry)

- *(md)* Wave 12m Phase 2 Step 5 — Markdown CrossRef carry visible body text

- *(core)* **BREAKING** Wave 12m Phase 2 Step 3 — foundation/core API breaking (RefType + RefContentType + RefTarget + Control::CrossRef target)

- *(hwp5)* **BREAKING** Wave 12j — 글자겹침(compose) carry + char_pr_ids fidelity + packed-variant 지원

- *(hwp5)* **BREAKING** Wave 12i — 덧말(dutmal) option carry + flat-path control_iter 필터

- *(hwp5)* **BREAKING** Wave 12f-h — 메모 anchor 위치 수정 + 7 parameters wire 메타 carry 완성

- *(hwp5)* **BREAKING** Wave 12e — 메모 본문 carry + 본문 텍스트 덮어쓰기 버그 수정


### Documentation

- Wave 12l + Phase 12 series 완료 반영 (CLAUDE/MEMORY/README)


### Fixed

- *(md)* capture GFM table header row (was silently dropped)

- *(hwpx)* Wave 12p task #124 — SUMMERY editable per FieldType + Wave 12p Step 4 visual gate + fmt fallout


## [0.5.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.4.0...hwpforge-smithy-md-v0.5.0) - 2026-03-22

### Added

- *(list)* **BREAKING** add checkable bullet semantics

- *(list)* **BREAKING** add shared list semantics


### Documentation

- refresh readme and fix docs lint


### Fixed

- *(md)* normalize ordered task lists

- *(md)* preserve task list continuations

- *(list)* restore markdown task lists and tighten bullet semantics

- *(hwpx)* bridge registry-local style ids


## [0.4.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.3.0...hwpforge-smithy-md-v0.4.0) - 2026-03-19

### Chore

- *(release)* **BREAKING** prepare v0.4.0 for tab semantics


## [0.3.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.2.1...hwpforge-smithy-md-v0.3.0) - 2026-03-18

### Chore

- *(release)* **BREAKING** prepare v0.3.0 for preserving section API changes


## [0.2.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.2.0...hwpforge-smithy-md-v0.2.1) - 2026-03-17

### Fixed

- *(docs)* unescape HTML entities in details/summary tags


## [0.2.0](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.7...hwpforge-smithy-md-v0.2.0) - 2026-03-17

### Changed

- Align the Markdown codec crate with the workspace-wide `0.2.0` release line.
- Update internal table construction to the `hwpforge-core` `0.2.0` builder-based contract.

## [0.1.7](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.6...hwpforge-smithy-md-v0.1.7) - 2026-03-12

### Added

- HWPX→Markdown styled conversion pipeline


### Documentation

- update HWPX→MD examples and README with to-md/from-json CLI commands


## [0.1.5](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.4...hwpforge-smithy-md-v0.1.5) - 2026-03-10

### Fixed

- *(dist)* improve user experience for npm installation


## [0.1.4](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.3...hwpforge-smithy-md-v0.1.4) - 2026-03-09

### Added

- *(examples)* add HWPX → Markdown conversion example


### Documentation

- change Anvil emoji from ⚒️ to ⚙️ for better semantic match

- add Bindings branding (Hammer/Anvil/Tongs), MCP multi-platform install guide, SKILL.md agent rules

- update README with MCP server section, badges, and project stats


## [0.1.3](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.2...hwpforge-smithy-md-v0.1.3) - 2026-03-09

### Added

- *(examples)* reorganize examples and add 16 HWPX showcase files


## [0.1.1](https://github.com/ai-screams/HwpForge/compare/hwpforge-smithy-md-v0.1.0...hwpforge-smithy-md-v0.1.1) - 2026-03-07

### Documentation

- *(readme)* add supported Hancom versions table and cargo install instructions

- update LICENSE-APACHE to full text and add README badges


### Fixed

- *(readme)* replace broken Buy Me a Coffee button with stable CDN image

- use absolute URLs for README images (crates.io compatibility)
