# HwpForge

> **Rust로 한글(HWP/HWPX) 문서를 프로그래밍 방식으로 제어**
>
> [Hancom](https://www.hancom.com/) 한글 파일 읽기, 쓰기, 변환

[![CI](https://img.shields.io/github/actions/workflow/status/ai-screams/HwpForge/ci.yml?branch=main&label=CI&logo=github)](https://github.com/ai-screams/HwpForge/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/hwpforge.svg?logo=rust)](https://crates.io/crates/hwpforge)
[![docs.rs](https://img.shields.io/docsrs/hwpforge?logo=docs.rs)](https://docs.rs/hwpforge)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![MSRV](https://img.shields.io/badge/MSRV-1.88+-orange.svg?logo=rust)](Cargo.toml)
[![codecov](https://img.shields.io/badge/coverage-92.65%25-brightgreen.svg?logo=codecov)](https://github.com/ai-screams/HwpForge)
[![Lines of Code](https://img.shields.io/badge/LOC-~49%2C200-informational.svg)](https://github.com/ai-screams/HwpForge)
[![Tests](https://img.shields.io/badge/tests-1%2C510_passed-success.svg?logo=checkmarx)](https://github.com/ai-screams/HwpForge)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success.svg?logo=rust)](https://github.com/ai-screams/HwpForge)

---

## HwpForge란?

HwpForge는 HWPX 문서(ZIP + XML, KS X 6101)를 다루기 위한 **오픈소스** 순수 Rust 라이브러리입니다. 한국에서 가장 많이 사용되는 워드프로세서인 [Hancom 한글](https://www.hancom.com)의 최신 포맷을 지원합니다.

**LLM-first 설계** — AI 에이전트가 Markdown으로 문서 내용을, YAML 스타일 템플릿으로 디자인을 정의하면 HwpForge가 HWPX로 컴파일합니다. Claude나 GPT 같은 LLM이 한글 문서를 프로그래밍 방식으로 생성하기에 적합한 구조입니다.

- **Full HWPX codec** — HWPX 파일을 손실 없이 디코딩/인코딩 (lossless roundtrip)
- **Markdown bridge** — GFM Markdown과 HWPX 간 상호 변환
- **YAML style template** — Figma Design Token처럼 재사용 가능한 스타일 정의 (폰트, 크기, 색상)
- **Type-safe API** — branded index, typestate validation, zero unsafe code

## 빠른 시작

`Cargo.toml`에 추가:

```toml
[dependencies]
hwpforge = "0.1"
```

### 문서 생성

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

### HWPX로 인코딩

```rust
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::core::ImageStore;

let validated = doc.validate().unwrap();
let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
let image_store = ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
std::fs::write("output.hwpx", &bytes).unwrap();
```

### HWPX 디코딩

```rust
use hwpforge::hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("input.hwpx").unwrap();
println!("섹션 수: {}", result.document.sections().len());
```

### Markdown → HWPX 변환

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

## 지원 콘텐츠

| 카테고리      | 요소                                                                 |
| ------------- | -------------------------------------------------------------------- |
| 텍스트        | Run, character shape, paragraph shape, style (22개 한컴 기본 스타일) |
| 구조          | Table (중첩), Image (바이너리 + 경로), TextBox, Caption              |
| 레이아웃      | 다단, 페이지 설정, 가로/세로 방향, 제본 여백, master page            |
| 머리글/바닥글 | Header, Footer, 쪽번호 (autoNum)                                     |
| 주석          | 각주, 미주                                                           |
| 도형          | 선, 타원, 다각형, 호, 곡선, 연결선 (채움, 회전, 화살표 지원)         |
| 수식          | HancomEQN script 형식                                                |
| 차트          | 18종 chart type (OOXML 호환)                                         |
| 참조          | 책갈피, 상호 참조, 필드 (날짜/시간/요약), 메모, 색인                 |
| 주석          | 덧말 (dutmal), 글자 겹침                                             |
| Markdown      | GFM decode, lossy + lossless encode, YAML frontmatter                |

## 아키텍처

```
hwpforge (umbrella crate)
  │
  ├── hwpforge-foundation    기본 타입: HwpUnit, Color (BGR), Index<T>
  ├── hwpforge-core          문서 모델: Section, Paragraph, Table, Shape
  ├── hwpforge-blueprint     YAML 스타일 템플릿 (상속 지원)
  ├── hwpforge-smithy-hwpx   HWPX codec (ZIP+XML, KS X 6101)
  └── hwpforge-smithy-md     Markdown codec (GFM + frontmatter)
```

**핵심 원칙**: 구조(Structure)와 스타일(Style)의 분리 — HTML + CSS와 같은 패턴입니다.
Core는 스타일 _참조_(index)만 가지고, Blueprint는 스타일 _정의_(폰트, 크기, 색상)를 관리합니다.
Smithy compiler가 Core + Blueprint를 합쳐 최종 포맷을 생성합니다.

## 프로젝트 현황

| 지표        | 값                      |
| ----------- | ----------------------- |
| 총 LOC      | ~49,200                 |
| 테스트      | 1,510개 (cargo-nextest) |
| 소스 파일   | 92 .rs                  |
| Crate 수    | 9개 (6개 배포)          |
| 커버리지    | 92.65%                  |
| Clippy 경고 | 0                       |
| Unsafe 코드 | 0                       |

## 개발

### 필수 요구사항

- Rust 1.88+ (MSRV)
- (권장) [cargo-nextest](https://nexte.st/) — 병렬 테스트 실행
- (선택) [pre-commit](https://pre-commit.com/) — git hook 자동화

### 명령어

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
│   └── hwpforge-bindings-cli/    # CLI 도구 (예정)
├── tests/                        # 통합 테스트 + golden fixture
└── examples/                     # 사용 예제
```

## 라이선스

다음 중 하나를 선택할 수 있습니다:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

## 감사의 말

이 프로젝트는 Hancom 한글(.hwp/.hwpx) 파일 포맷의 공개 문서를 참조합니다.
HWPX 포맷은 KS X 6101 (OWPML) 국가 표준을 따릅니다.
