# 아키텍처 개요

HwpForge는 **대장간(Forge) 메타포**를 기반으로 설계된 계층형 크레이트 구조를 갖습니다.
각 계층은 명확한 역할을 가지며, 상위 계층은 하위 계층에만 의존합니다.

## Forge 메타포

| 계층               | 역할                    | 비유               |
| ------------------ | ----------------------- | ------------------ |
| Foundation (기반)  | 원시 타입, 단위, 인덱스 | 쇠못과 금속 소재   |
| Core (핵심)        | 형식 독립 문서 모델     | 도면 위의 설계도   |
| Blueprint (청사진) | YAML 스타일 템플릿      | 피그마 디자인 토큰 |
| Smithy (대장간)    | 형식별 인코더/디코더    | 용광로와 망치      |
| Bindings (바인딩)  | Python, CLI 인터페이스  | 완성된 제품 포장   |

## 크레이트 의존성 그래프

```mermaid
graph TD
    F[hwpforge-foundation<br/>원시 타입] --> C[hwpforge-core<br/>문서 모델]
    C --> B[hwpforge-blueprint<br/>스타일 템플릿]
    B --> SH[hwpforge-smithy-hwpx<br/>HWPX 코덱]
    B --> SM[hwpforge-smithy-md<br/>Markdown 코덱]
    B --> S5[hwpforge-smithy-hwp5<br/>HWP5 디코더 (예정)]
    SH --> U[hwpforge<br/>umbrella crate]
    SM --> U
    S5 --> U
    U --> PY[hwpforge-bindings-py<br/>Python (예정)]
    U --> CLI[hwpforge-bindings-cli<br/>CLI (예정)]
```

> **규칙**: 의존성은 위에서 아래로만 흐릅니다. `foundation`을 수정하면 모든 크레이트가 재빌드됩니다.
> 따라서 `foundation`은 최소한으로 유지합니다.

## 핵심 원칙: 구조와 스타일의 분리

HwpForge는 HTML + CSS의 관계처럼 **문서 구조**와 **스타일 정의**를 완전히 분리합니다.

```
Core (구조)           Blueprint (스타일)
─────────────         ──────────────────
Paragraph             font: "맑은 고딕"
  style_id: 2    ──▶  size: 10pt
  runs: [...]         color: #000000
```

- **Core**는 스타일 ID(인덱스)만 보유합니다. 실제 글꼴 이름이나 크기를 모릅니다.
- **Blueprint**는 스타일 정의를 YAML 템플릿으로 관리합니다.
- **Smithy** 컴파일러가 Core + Blueprint를 조합해 최종 형식을 생성합니다.

이 구조 덕분에 하나의 YAML 템플릿을 여러 문서에 재사용하거나,
동일한 문서를 HWPX/Markdown 등 다른 형식으로 내보낼 수 있습니다.

## 타입스테이트 패턴: Document<Draft> → Document<Validated>

`Document`는 컴파일 타임에 상태를 추적하는 타입스테이트 패턴을 사용합니다.

```rust,no_run
use hwpforge::core::{Document, Draft};

// Draft 상태: 편집 가능, 저장 불가
let mut doc = Document::<Draft>::new();
doc.add_section(/* ... */);

// validate()를 호출해야만 Validated 상태로 전이
let validated = doc.validate().unwrap();

// Validated 상태에서만 인코딩 가능
// doc.validate()를 건너뛰면 컴파일 에러 발생
let bytes = hwpforge::hwpx::HwpxEncoder::encode(
    &validated,
    &hwpforge::hwpx::HwpxStyleStore::with_default_fonts("함초롬바탕"),
    &hwpforge::core::ImageStore::new(),
).unwrap();
```

잘못된 상태에서 저장을 시도하면 **런타임 에러가 아닌 컴파일 에러**가 발생합니다.

## 이중 포맷 설계: HWP5 + HWPX

한국에는 두 가지 주요 문서 포맷이 있습니다:

- **HWP5** (`.hwp`): OLE2/CFB 바이너리 컨테이너 + TLV 레코드 (1990년대~현재, 레거시)
- **HWPX** (`.hwpx`): ZIP 컨테이너 + XML 파일 (KS X 6101 국가 표준, 2014년~현재)

HwpForge는 **Core DOM이 포맷에 독립적**이도록 설계하여 두 포맷을 통합 처리합니다:

```text
HWP5 (.hwp)  ──decode──▶ ┌────────────────────┐ ◀──decode── Markdown (.md)
                          │  Document<Draft>   │
HWPX (.hwpx) ──decode──▶ │  (포맷 독립 IR)    │ ──encode──▶ HWPX / Markdown
                          └────────────────────┘
```

모든 Smithy 크레이트는 Core DOM으로/에서 변환만 수행합니다. 비즈니스 로직은 Core에만 의존하므로, 새 포맷(예: smithy-odt)을 추가해도 기존 코드를 수정할 필요가 없습니다.

> 자세한 내용은 [HWP5와 HWPX: 이중 포맷 파이프라인](../guide/format-pipeline.md)을 참고하세요.

## 각 크레이트 설명

### `hwpforge-foundation`

의존성이 없는 루트 크레이트입니다. 모든 크레이트가 공유하는 원시 타입을 정의합니다.

- **`HwpUnit`**: 정수 기반 HWP 단위 (1pt = 100 HWPUNIT). 부동소수점 오차 없음
- **`Color`**: BGR 바이트 순서 색상 타입. `Color::from_rgb(r, g, b)`로 생성
- **`Index<T>`**: 팬텀 타입을 이용한 브랜드 인덱스. `CharShapeIndex`와 `ParaShapeIndex`를 혼용하면 컴파일 에러

### `hwpforge-core`

형식에 독립적인 문서 모델입니다. 한글/Markdown/PDF 어디에도 종속되지 않습니다.

- `Document<S>`, `Section`, `Paragraph`, `Run` — 기본 문서 구조
- `Table`, `Control`, `Shape` — 복합 요소
- `PageSettings` — 용지 크기, 여백, 가로/세로 방향

### `hwpforge-blueprint`

YAML로 작성하는 스타일 템플릿 시스템입니다. 피그마의 디자인 토큰 개념과 유사합니다.

- 상속(`extends`)과 병합(merge)을 지원하는 `PartialCharShape` / `CharShape` 두 타입 구조
- `StyleRegistry` — 파싱 후 인덱스를 할당한 최종 스타일 집합

### `hwpforge-smithy-hwpx`

HWPX ↔ Core 변환을 담당하는 핵심 코덱입니다. KS X 6101(OWPML) 국가 표준을 구현합니다.

- `HwpxDecoder` — ZIP + XML 파싱 → Core 문서 모델
- `HwpxEncoder` — Core 문서 모델 → ZIP + XML 바이트
- `HwpxStyleStore` — 한컴 기본 스타일 22종(Modern) 내장

### `hwpforge-smithy-md`

GFM(GitHub Flavored Markdown) ↔ Core 변환을 담당합니다.

- `MdDecoder` — Markdown + YAML 프론트매터 → Core
- `MdEncoder` — Core → Markdown (손실/무손실 모드)

### `hwpforge` (umbrella crate)

모든 공개 크레이트를 재내보내기(re-export)하는 진입점 크레이트입니다.
사용자는 이 크레이트 하나만 의존성에 추가하면 됩니다.

## 다음 단계

- [빠른 시작](./quickstart.md) — 코드 예제로 API 체험
- [설치](./installation.md) — Feature flag 선택 방법
