# HwpForge 🔥

> **Rust로 한글(HWP/HWPX) 문서를 프로그래밍 방식으로 제어**
>
> [Hancom](https://www.hancom.com/) 한글 파일 읽기, 쓰기, 변환

<div align="center">

[![CI](https://img.shields.io/github/actions/workflow/status/ai-screams/HwpForge/ci.yml?branch=main&label=CI&logo=github)](https://github.com/ai-screams/HwpForge/actions/workflows/ci.yml)
[![codecov](https://img.shields.io/badge/coverage-92.65%25-brightgreen.svg?logo=codecov)](https://github.com/ai-screams/HwpForge)
[![Tests](https://img.shields.io/badge/tests-1%2C602_passed-success.svg?logo=checkmarx)](https://github.com/ai-screams/HwpForge)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg?logo=rust)](https://github.com/ai-screams/HwpForge)
[![Lines of Code](https://img.shields.io/badge/LOC-~52%2C700-informational.svg)](https://github.com/ai-screams/HwpForge)

[![crates.io](https://img.shields.io/crates/v/hwpforge.svg?logo=rust)](https://crates.io/crates/hwpforge)
[![docs.rs](https://img.shields.io/docsrs/hwpforge?logo=docs.rs)](https://docs.rs/hwpforge)
[![crates.io downloads](https://img.shields.io/crates/d/hwpforge.svg?label=downloads&logo=rust&color=orange)](https://crates.io/crates/hwpforge)
[![MSRV](https://img.shields.io/badge/MSRV-1.88+-orange.svg?logo=rust)](Cargo.toml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

[![MCP Ready](https://img.shields.io/badge/MCP-ready-blueviolet.svg?logo=data:image/svg%2bxml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHZpZXdCb3g9IjAgMCAyNCAyNCI+PHBhdGggZD0iTTEyIDJMMiA3bDEwIDVMMjIgN3oiIGZpbGw9IndoaXRlIi8+PHBhdGggZD0iTTIgMTdsMTAgNSAxMC01IiBmaWxsPSJ3aGl0ZSIgb3BhY2l0eT0iMC43Ii8+PHBhdGggZD0iTTIgMTJsMTAgNSAxMC01IiBmaWxsPSJ3aGl0ZSIgb3BhY2l0eT0iMC44NSIvPjwvc3ZnPg==)](https://modelcontextprotocol.io/)
[![GitHub release](https://img.shields.io/github/v/release/ai-screams/HwpForge?logo=github&color=green)](https://github.com/ai-screams/HwpForge/releases)
[![GitHub last commit](https://img.shields.io/github/last-commit/ai-screams/HwpForge?logo=github)](https://github.com/ai-screams/HwpForge)
[![GitHub stars](https://img.shields.io/github/stars/ai-screams/HwpForge?style=social)](https://github.com/ai-screams/HwpForge)

[![Security Policy](https://img.shields.io/badge/security-policy-blueviolet.svg?logo=githubactions)](SECURITY.md)
[![Contributing](https://img.shields.io/badge/contributing-guide-blue.svg?logo=handshake)](CONTRIBUTING.md)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg?logo=github)](https://github.com/ai-screams/HwpForge/pulls)
[![Made in Korea](https://img.shields.io/badge/made_in-Korea_🇰🇷-red.svg)](https://github.com/ai-screams/HwpForge)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-support-yellow.svg?logo=buy-me-a-coffee&logoColor=white)](https://buymeacoffee.com/pignuante)

</div>

<div align="center">
<img src="https://raw.githubusercontent.com/ai-screams/HwpForge/main/assets/banner-main.png" alt="HwpForge Banner" width="600">
</div>

---

## HwpForge란?

HwpForge는 HWPX 문서(ZIP + XML, KS X 6101)를 다루기 위한 **오픈소스** 순수 Rust 라이브러리입니다. 한국에서 가장 많이 사용되는 워드프로세서인 [Hancom 한글](https://www.hancom.com)의 최신 포맷을 지원합니다.

### 지원 버전

| 한글 버전        | 포맷         | 읽기    | 쓰기 | 스타일 세트                    |
| ---------------- | ------------ | ------- | ---- | ------------------------------ |
| 한글 2014 ~ 2020 | HWPX (.hwpx) | ✅      | ✅   | Classic (18 styles)            |
| 한글 2022 ~ 2024 | HWPX (.hwpx) | ✅      | ✅   | Modern (22 styles, **기본값**) |
| 한글 2025+       | HWPX (.hwpx) | ✅      | ✅   | Latest (23 styles)             |
| 한글 97 ~ 2010   | HWP5 (.hwp)  | 📋 예정 | —    | —                              |

- **HWPX**: OWPML 국가표준 (KS X 6101) 기반, ZIP + XML 컨테이너
- **HWP5**: 구형 바이너리 포맷 (v2.0에서 읽기 지원 예정)
- 스타일 세트는 `HancomStyleSet` enum으로 선택 가능 (기본: Modern)

**LLM-first 설계** 🔥 — AI 친화적인 Markdown과 공식 한글 문서 포맷(HWPX), 두 세계를 자연스럽게 잇습니다. LLM이 Markdown으로 작성한 내용은 공문서 규격의 HWPX로 컴파일되고 📜, 반대로 기존 HWPX 문서는 AI가 쉽게 읽을 수 있는 구조로 꺼낼 수 있습니다 ⚒️.

- **📄 [HWPX 완전 가이드 다운로드](examples/hwpx_complete_guide.hwpx)** — HwpForge API로 생성한 4섹션 데모 문서 (한글에서 열어보세요)
- **HWPX Reader for AI** — 기존 한글 문서(.hwpx)를 Markdown으로 변환하여 LLM이 즉시 이해 가능
- **Full HWPX codec** — HWPX 파일을 손실 없이 디코딩/인코딩 (lossless roundtrip)
- **Markdown bridge** — GFM Markdown과 HWPX 간 양방향 변환 (읽기 + 쓰기)
- **YAML style template** — Figma Design Token처럼 재사용 가능한 스타일 정의 (폰트, 크기, 색상)
- **Type-safe API** — branded index, typestate validation, zero unsafe code

## 빠른 시작

### 설치

```bash
# Cargo.toml에 추가
cargo add hwpforge

# Markdown 지원 포함
cargo add hwpforge --features full
```

또는 `Cargo.toml`에 직접 추가:

```toml
[dependencies]
hwpforge = "0.1"
```

### 🔨 Hammer — CLI로 시작하기

CLI 도구 `hwpforge`(Hammer)를 설치하면 터미널에서 바로 문서를 생성하고 편집할 수 있습니다.

```bash
cargo install hwpforge-bindings-cli
```

```bash
# Markdown → HWPX 변환
hwpforge convert report.md -o report.hwpx

# HWPX 구조 확인
hwpforge inspect report.hwpx

# HWPX → JSON 추출 (AI 편집용)
hwpforge to-json report.hwpx --section 0 > section0.json

# JSON으로 섹션 교체
hwpforge patch report.hwpx --section 0 < modified.json -o updated.hwpx

# JSON Schema 출력 (AI agent용)
hwpforge schema document
```

> **AI-first 설계**: CLI는 AI agent(Claude Code 등)가 주 사용자입니다.
> Markdown으로 문서를 생성한 뒤, JSON round-trip으로 기존 스타일을 보존하면서
> section 단위로 정밀하게 편집할 수 있습니다. `--json` 플래그로 모든 명령어가
> machine-readable 출력을 지원합니다.

### ⚙️ Anvil — MCP Server로 AI가 직접 한글 문서를 다루다

Claude Code, Codex CLI, Claude, ChatGPT, Cursor, Antigravity 등 [MCP](https://modelcontextprotocol.io/) 지원 AI 도구에서 **한글 문서를 직접 생성하고 편집**할 수 있습니다. "보고서 만들어줘"라고 말하면, AI가 알아서 `.hwpx` 파일을 뚝딱 만들어냅니다.

#### AI 도구에 등록

한 줄이면 설치 + 등록이 끝납니다. npm은 `npx -y`가 자동으로 바이너리를 다운로드합니다.

<details>
<summary><strong>Claude Code</strong> (터미널)</summary>

```bash
# npm (권장 — Rust 툴체인 불필요)
claude mcp add hwpforge -- npx -y @hwpforge/mcp

# Cargo (Rust 개발자용)
cargo install hwpforge-bindings-mcp && claude mcp add hwpforge hwpforge-mcp

# 모든 프로젝트에서 사용 (글로벌)
claude mcp add --global hwpforge -- npx -y @hwpforge/mcp
```

</details>

<details>
<summary><strong>Codex CLI</strong> (터미널)</summary>

`~/.codex/config.toml`에 추가:

```toml
[mcp_servers.hwpforge]
command = "npx"
args = ["-y", "@hwpforge/mcp"]
```

또는 CLI로:

```bash
codex mcp add hwpforge -- npx -y @hwpforge/mcp
```

</details>

<details>
<summary><strong>Claude Desktop</strong> (앱)</summary>

설정 파일을 편집합니다:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

</details>

<details>
<summary><strong>ChatGPT Desktop</strong> (앱)</summary>

Settings → Tools → Add MCP Server에서:

- Name: `hwpforge`
- Command: `npx -y @hwpforge/mcp`

또는 설정 파일을 직접 편집:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

</details>

<details>
<summary><strong>Cursor</strong> (에디터)</summary>

프로젝트 루트에 `.cursor/mcp.json` 생성:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

</details>

<details>
<summary><strong>Antigravity</strong> (에디터)</summary>

`...` 드롭다운 → MCP Store → Manage MCP Servers → View raw config (`mcp_config.json`)에 추가:

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

</details>

#### 등록하면 5개 도구를 사용할 수 있습니다

| 도구                 | 하는 일              | 한마디                         |
| -------------------- | -------------------- | ------------------------------ |
| `hwpforge_convert`   | Markdown → HWPX 변환 | "이 마크다운을 한글 파일로!"   |
| `hwpforge_inspect`   | HWPX 구조 확인       | "이 문서 뭐가 들어있어?"       |
| `hwpforge_to_json`   | HWPX → JSON 추출     | "이 섹션 내용 좀 꺼내봐"       |
| `hwpforge_patch`     | JSON으로 섹션 교체   | "이 부분만 바꿔서 다시 저장해" |
| `hwpforge_templates` | 스타일 프리셋 조회   | "어떤 템플릿 쓸 수 있어?"      |

#### 업데이트 / 삭제

npm은 `npx -y`가 항상 최신 버전을 가져오므로 별도 업데이트가 필요 없습니다.

```bash
# Cargo 사용자만 해당
cargo install hwpforge-bindings-mcp --force   # 업데이트
cargo uninstall hwpforge-bindings-mcp          # 삭제
```

> **왜 MCP?** CLI(Hammer)는 AI가 `bash` 명령을 실행해야 하지만, MCP(Anvil)는 AI가 **네이티브 도구**로
> 직접 호출합니다. 파일 경로 파싱도, stdout 해석도 필요 없습니다.
> JSON-RPC로 요청하면 구조화된 JSON으로 응답 — 깔끔합니다.

### 🔨 문서 생성

```rust
use hwpforge::core::{Document, Draft, Paragraph, Run, Section, PageSettings};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};

let mut doc = Document::<Draft>::new();
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::text("Hello, 한글!", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));
```

### ⚒️ HWPX로 인코딩

```rust
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::core::ImageStore;

let validated = doc.validate().unwrap();
let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
let image_store = ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
std::fs::write("output.hwpx", &bytes).unwrap();
```

### ⚒️ HWPX 디코딩

```rust
use hwpforge::hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("input.hwpx").unwrap();
println!("섹션 수: {}", result.document.sections().len());
```

### ⚒️ HWPX → Markdown 변환 (AI가 한글 문서 읽기)

```rust
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;

let decoded = HwpxDecoder::decode_file("government_report.hwpx").unwrap();
let validated = decoded.document.validate().unwrap();
let markdown = MdEncoder::encode_lossy(&validated).unwrap();
println!("{}", markdown); // LLM이 바로 이해할 수 있는 Markdown
```

기존 .hwpx 파일을 Markdown으로 변환하면 Claude, GPT 등 어떤 LLM이든 한글 공문서를 즉시 읽고 분석할 수 있습니다.

### ⚒️ Markdown → HWPX 변환

```rust
use hwpforge::md::MdDecoder;
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};

let md_doc = MdDecoder::decode_with_default("# 제목\n\nMarkdown에서 변환!").unwrap();
let validated = md_doc.document.validate().unwrap();
let style_store = HwpxStyleStore::from_registry(&md_doc.style_registry);
let image_store = hwpforge::core::ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
```

## Feature Flags

| Feature | 기본값 | 설명                 |
| ------- | ------ | -------------------- |
| `hwpx`  | Yes    | HWPX encoder/decoder |
| `md`    | —      | Markdown ↔ Core 변환 |
| `full`  | —      | 모든 기능 포함       |

```toml
# Markdown 지원 포함
hwpforge = { version = "0.1", features = ["full"] }
```

## 📜 지원 콘텐츠

| 카테고리      | 요소                                                                 |
| ------------- | -------------------------------------------------------------------- |
| 텍스트        | Run, character shape, paragraph shape, style (22개 한컴 기본 스타일) |
| 구조          | Table (중첩), Image (바이너리 + 경로), TextBox, Caption              |
| 레이아웃      | 다단, 페이지 설정, 가로/세로 방향, 제본 여백, master page            |
| 머리글/바닥글 | Header, Footer, 쪽번호 (autoNum)                                     |
| 각주/미주     | 각주, 미주                                                           |
| 도형          | 선, 타원, 다각형, 호, 곡선, 연결선 (채움, 회전, 화살표 지원)         |
| 수식          | HancomEQN script 형식                                                |
| 차트          | 18종 chart type (OOXML 호환)                                         |
| 참조          | 책갈피, 상호 참조, 필드 (날짜/시간/요약), 메모, 색인                 |
| 덧말/겹침     | 덧말 (dutmal), 글자 겹침                                             |
| Markdown      | GFM decode, lossy + lossless encode, YAML frontmatter                |

## 아키텍처

```mermaid
%%{init: {'theme': 'base', 'themeVariables': {'fontSize': '14px', 'lineColor': '#BDBDBD'}}}%%
flowchart TD
    subgraph formats["포맷"]
        HF(["📄 .hwpx<br/>한글 파일"]):::file
        MF(["📝 .md<br/>Markdown"]):::file
    end

    subgraph smithy["⚒️ Smithy — 포맷 변환기"]
        SH["hwpforge-smithy-hwpx"]:::smithy
        SM["hwpforge-smithy-md"]:::smithy
    end

    C["🔨 hwpforge-core<br/>포맷 독립 문서 모델 (IR)"]:::core
    BP["📐 hwpforge-blueprint<br/>YAML 스타일 · 폰트 · 색상"]:::blueprint
    F["🔩 hwpforge-foundation<br/>HwpUnit · Color · Index"]:::foundation

    HF <--> SH
    MF <--> SM
    SH & SM <--> C
    BP --> SH & SM
    F --> C & BP

    classDef file     fill:#FFFDE7,stroke:#F9A825,color:#5D4037
    classDef smithy   fill:#FFF3E0,stroke:#FB8C00,color:#E65100
    classDef core     fill:#E3F2FD,stroke:#42A5F5,color:#0D47A1
    classDef blueprint fill:#F3E5F5,stroke:#AB47BC,color:#4A148C
    classDef foundation fill:#FAFAFA,stroke:#BDBDBD,color:#424242
```

**핵심 원칙**: 구조(Structure)와 스타일(Style)의 분리 — HTML + CSS와 같은 패턴입니다.
Core는 스타일 _참조_(index)만 가지고, Blueprint는 스타일 _정의_(폰트, 크기, 색상)를 관리합니다.
Smithy compiler가 Core + Blueprint를 합쳐 최종 포맷을 생성합니다.

## 프로젝트 현황

| 지표        | 값                      |
| ----------- | ----------------------- |
| 총 LOC      | ~52,700                 |
| 테스트      | 1,602개 (cargo-nextest) |
| 소스 파일   | 116 .rs                 |
| Crate 수    | 10개 (7개 배포)         |
| 커버리지    | 92.65%                  |
| Clippy 경고 | 0                       |
| Unsafe 코드 | 0                       |

## 개발

### 필수 요구사항

- Rust 1.88+ (MSRV)
- (권장) [cargo-nextest](https://nexte.st/) — 병렬 테스트 실행
- (선택) [pre-commit](https://pre-commit.com/) — git hook 자동화

### MSRV 정책

- 현재 MSRV는 **Rust 1.88**입니다.
- HwpForge는 **stable에서 4 릴리스 뒤처진 버전**을 기본 MSRV 정책으로 유지합니다.
- `Cargo.toml`의 `rust-version`이 단일 진실원이며, CI의 `Verify › MSRV` job이 이 계약을 검증합니다.
- MSRV 상향이 필요하면 PR에서 이유를 명시하고, `Cargo.toml`, CI, CHANGELOG를 함께 갱신합니다.
- 개발용 기본 툴체인은 더 최신일 수 있습니다. 호환성 판단 기준은 최신 stable이 아니라 **MSRV + CI 통과 여부**입니다.

### ⚒️ 명령어

```bash
make ci          # fmt + clippy + test + deny + lint (CI와 동일)
make test        # cargo nextest run
make clippy      # cargo clippy (모든 target, 모든 feature, -D warnings)
make fmt-fix     # rustfmt 자동 포맷
make doc         # rustdoc 생성 (브라우저에서 열림)
make cov         # coverage 리포트 (90% gate)
```

### 프로젝트 구조

```
HwpForge/
├── crates/
│   ├── hwpforge/                 # Umbrella crate (re-exports)
│   ├── hwpforge-foundation/      # 기본 타입 (HwpUnit, Color, Index<T>)
│   ├── hwpforge-core/            # 문서 모델 (스타일 참조만)
│   ├── hwpforge-blueprint/       # YAML 템플릿 (Figma 패턴)
│   ├── hwpforge-smithy-hwpx/     # HWPX codec (ZIP+XML ↔ Core)
│   ├── hwpforge-smithy-md/       # Markdown codec (MD ↔ Core)
│   ├── hwpforge-smithy-hwp5/     # HWP5 decoder (예정)
│   ├── hwpforge-bindings-py/     # Python bindings (예정)
│   ├── hwpforge-bindings-cli/    # CLI 도구 (hwpforge)
│   └── hwpforge-bindings-mcp/    # MCP Server (hwpforge-mcp)
├── tests/                        # 통합 테스트 + golden fixture
└── examples/                     # 📜 사용 예제 + 생성된 HWPX 파일
```

## 기여

버그 수정, 포맷 리서치, 테스트 보강, 문서 개선 모두 환영합니다.

- 시작 전 가이드: [CONTRIBUTING.md](CONTRIBUTING.md)
- 특히 확인할 것: release-plz가 쓰는 커밋 prefix (`feat`, `fix`, `perf`, `refactor`)
- 특히 확인할 것: MSRV 정책과 dependency/MSRV 상승 기준
- 특히 확인할 것: 문서 변경 시 `mdbook build`와 markdown lint 검증
- 특히 확인할 것: CI required checks를 깨지 않는 범위에서의 변경 분리

## 로드맵

### 출시 예정

- [ ] HWP5 읽기 — 구형 바이너리 포맷(`.hwp`) 디코더
- [x] MCP 서버 — Claude, Cursor 등 AI 도구가 tool로 직접 HWPX 생성 (5개 도구)
- [x] CLI 도구 — `hwpforge convert doc.md doc.hwpx` 한 줄 변환 (7개 명령어)

- [ ] HWPX 완전 지원 — 양식 컨트롤, 변경 추적, OLE 객체
- [ ] Python 바인딩 — `pip install hwpforge`로 설치, PyPI 배포

## 라이선스

다음 중 하나를 선택할 수 있습니다:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

## Acknowledgements

HwpForge는 거인들의 어깨 위에 서 있습니다.

- **[Hancom](https://www.hancom.com)** — HWPX 포맷의 공개 문서와 [KS X 6101 (OWPML)](https://www.kssn.net/) 국가 표준이 없었다면 이 프로젝트는 시작조차 할 수 없었습니다. 포맷을 공개해 주신 Hancom에 감사드립니다.

- **[openhwp](https://github.com/openhwp/openhwp)** — Rust로 HWP/HWPX를 다루는 IR 기반 아키텍처 설계에서 큰 영감을 받았습니다. HwpForge의 Core 레이어가 존재할 수 있었던 것은 openhwp이 먼저 그 길을 걸었기 때문입니다.

- **[hwpxlib](https://github.com/neolord0/hwpxlib)** — Java로 작성된 가장 성숙한 HWPX 구현체입니다. 스펙과 실제 동작의 차이를 파악하는 데 결정적인 참고가 되었습니다.

- **[hwp.js](https://github.com/hahnlee/hwp.js)** — HWP5 포맷의 quirks와 edge case를 꼼꼼히 문서화한 프로젝트입니다. 바이너리 포맷의 어두운 구석을 밝혀 준 덕분에 시행착오를 크게 줄일 수 있었습니다.

- **[hwpx-owpml-model](https://github.com/hancom-io/hwpx-owpml-model)** — Hancom이 직접 공개한 C++ OWPML 모델 구현체로, 스키마 해석의 최종 기준으로 삼았습니다.

- **Rust 생태계** — [serde](https://serde.rs), [quick-xml](https://github.com/tafia/quick-xml), [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark), [zip](https://github.com/zip-rs/zip2) 등 뛰어난 라이브러리들 덕분에 HwpForge 전체를 zero unsafe 순수 Rust로 구현할 수 있었습니다. Rust 커뮤니티와 Ferris 🦀에게 감사드립니다.

- **[Claude](https://claude.ai) by [Anthropic](https://www.anthropic.com)** — HwpForge의 설계, 구현, 테스트, 문서화 전 과정에서 Claude Code가 개발 파트너로 함께했습니다. LLM-first를 표방하는 프로젝트답게, AI와 사람이 협업하여 만들어낸 결과물입니다.

---

<div align="center">
<img src="https://raw.githubusercontent.com/ai-screams/HwpForge/main/assets/mascot-main.png" width="260" alt="쇠부리 Anvilscribe (SoeBuri Anvilscribe)">

<br/><br/>

<strong>쇠부리 Anvilscribe (SoeBuri Anvilscribe)</strong><br/>
<em>한컴 문서를 불에 달구어 단단하게 벼려내는 대장장이 오리너구리 🔥</em>

<br/><br/>

<a href="https://buymeacoffee.com/pignuante">
<img src="https://cdn.buymeacoffee.com/buttons/v2/default-yellow.png" alt="Buy Me a Coffee" height="50" width="217">
</a>

</div>
