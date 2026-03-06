# HwpForge

**한글 문서(HWP/HWPX)를 프로그래밍으로 제어하는 Rust 라이브러리**

[![crates.io](https://img.shields.io/crates/v/hwpforge.svg)](https://crates.io/crates/hwpforge)
[![docs.rs](https://docs.rs/hwpforge/badge.svg)](https://docs.rs/hwpforge)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/ai-screams/HwpForge)

---

## HwpForge란?

HwpForge는 [한컴 한글](https://www.hancom.com/)의 HWPX 문서(ZIP + XML, KS X 6101)를 Rust로 읽고, 쓰고, 변환할 수 있는 라이브러리입니다.

### 주요 기능

- **HWPX 풀 코덱** — HWPX 파일 디코딩/인코딩 + 무손실 라운드트립
- **Markdown 브릿지** — GFM Markdown ↔ HWPX 변환
- **YAML 스타일 템플릿** — 재사용 가능한 디자인 토큰 (Figma 패턴)
- **타입 안전 API** — 브랜드 인덱스, 타입스테이트 검증, unsafe 코드 0

### 지원 콘텐츠

| 카테고리      | 요소                                                  |
| ------------- | ----------------------------------------------------- |
| 텍스트        | 런, 문자 모양, 문단 모양, 스타일 (한컴 기본 22종)     |
| 구조          | 표 (중첩), 이미지, 글상자, 캡션                       |
| 레이아웃      | 다단, 페이지 설정, 가로/세로, 여백, 마스터페이지      |
| 머리글/바닥글 | 머리글, 바닥글, 쪽번호 (autoNum)                      |
| 주석          | 각주, 미주                                            |
| 도형          | 선, 타원, 다각형, 호, 곡선, 연결선 (채움/회전/화살표) |
| 수식          | HancomEQN 스크립트                                    |
| 차트          | 18종 차트 (OOXML 호환)                                |
| 참조          | 책갈피, 상호참조, 필드, 메모, 찾아보기                |
| Markdown      | GFM 디코드, 손실/무손실 인코드, YAML 프론트매터       |

### 누구를 위한 라이브러리인가?

- **LLM/AI 에이전트** — 자연어로 한글 문서 자동 생성
- **백엔드 개발자** — 서버에서 한글 문서 프로그래밍 생성
- **자동화 도구** — CI/CD 파이프라인에서 보고서 자동 생성
- **데이터 파이프라인** — HWPX 문서에서 텍스트/표 추출

## 빠른 맛보기

```rust,no_run
use hwpforge::core::{Document, Draft, Paragraph, Run, Section, PageSettings};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::core::ImageStore;

// 1. 문서 생성
let mut doc = Document::<Draft>::new();
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::text("안녕하세요, HwpForge!", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));

// 2. 검증 + 인코딩
let validated = doc.validate().unwrap();
let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
let image_store = ImageStore::new();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();

// 3. 파일 저장
std::fs::write("output.hwpx", &bytes).unwrap();
```

## 다음 단계

- [설치](getting-started/installation.md) — Cargo.toml에 추가하기
- [빠른 시작](getting-started/quickstart.md) — 10분 안에 첫 HWPX 생성
- [아키텍처 개요](getting-started/architecture.md) — 크레이트 구조 이해
- [API 레퍼런스](https://docs.rs/hwpforge) — 전체 API 문서 (docs.rs)
