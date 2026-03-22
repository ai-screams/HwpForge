# HWP5와 HWPX: 이중 포맷 파이프라인

HwpForge는 한국의 두 가지 주요 문서 포맷 — 바이너리 OLE 기반 **HWP5**와 XML 기반 **HWPX** — 을 하나의 통합 파이프라인으로 처리할 수 있도록 설계되었습니다.

## 포맷 비교

| 특성              | HWP5 (`.hwp`)                                   | HWPX (`.hwpx`)                                                |
| ----------------- | ----------------------------------------------- | ------------------------------------------------------------- |
| **컨테이너**      | OLE2/CFB (Compound File Binary)                 | ZIP                                                           |
| **내부 데이터**   | 바이너리 레코드 스트림                          | XML 파일 (KS X 6101 OWPML)                                    |
| **표준**          | 한컴 독자 포맷 (공개 스펙)                      | 국가 표준 KS X 6101                                           |
| **역사**          | 1990년대~현재 (레거시)                          | 2014년~ (현대)                                                |
| **파일 시그니처** | `D0 CF 11 E0 A1 B1 1A E1` (OLE)                 | `50 4B 03 04` (ZIP/PK)                                        |
| **스트림 구조**   | `FileHeader`, `DocInfo`, `BodyText/Section0` 등 | `mimetype`, `Contents/header.xml`, `Contents/section0.xml` 등 |
| **압축**          | zlib (스트림 단위)                              | ZIP deflate (파일 단위)                                       |
| **암호화**        | 지원 (스트림 암호화)                            | 지원 (ZIP 암호화)                                             |
| **한글 호환성**   | 한글 97~최신                                    | 한글 2014~최신                                                |

## Core DOM: 포맷 독립 중간 표현 (IR)

HwpForge의 핵심 설계 원칙은 **Core DOM이 포맷에 독립적**이라는 것입니다. `Document<Draft>`는 HWP5든 HWPX든 Markdown이든 동일한 구조체로 표현됩니다.

```text
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  HWP5 파일  │     │  HWPX 파일  │     │  Markdown   │
│ (OLE/CFB)   │     │ (ZIP/XML)   │     │ (GFM+YAML)  │
└──────┬──────┘     └──────┬──────┘     └──────┬──────┘
       │ decode            │ decode            │ decode
       ▼                   ▼                   ▼
┌──────────────────────────────────────────────────────┐
│              Document<Draft>  (Core DOM)              │
│  ┌──────────────────────────────────────────────┐    │
│  │ Sections → Paragraphs → Runs → Text/Control  │    │
│  │ + Metadata (title, author, created, ...)      │    │
│  │ + Tables, Images, Shapes, Charts, ...         │    │
│  └──────────────────────────────────────────────┘    │
│              포맷 독립 중간 표현 (IR)                  │
└──────────┬───────────────┬───────────────┬───────────┘
           │ encode        │ encode        │ encode
           ▼               ▼               ▼
    ┌──────────┐    ┌──────────┐    ┌──────────┐
    │ HWP5     │    │ HWPX     │    │ Markdown │
    │ (예정)   │    │ ✅ 구현   │    │ ✅ 구현   │
    └──────────┘    └──────────┘    └──────────┘
```

이 설계 덕분에:

- **하나의 문서 모델**로 모든 포맷을 처리합니다
- 포맷 간 **변환**이 Core DOM을 경유하여 자연스럽게 이루어집니다
- 새 포맷 추가 시 기존 코드 수정 없이 **Smithy 크레이트만 추가**하면 됩니다
- 비즈니스 로직은 **Core DOM에만 의존**하므로 포맷 변경에 영향을 받지 않습니다

## 포맷 감지

파일의 첫 바이트(매직 바이트)로 포맷을 판별합니다.

```rust,no_run
/// 파일 포맷 감지
enum DocumentFormat {
    Hwp5,     // OLE2/CFB 바이너리
    Hwpx,     // ZIP + XML
    Markdown, // 텍스트
    Unknown,
}

fn detect_format(bytes: &[u8]) -> DocumentFormat {
    if bytes.len() < 4 {
        return DocumentFormat::Unknown;
    }

    // OLE2 Compound File Binary: D0 CF 11 E0
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return DocumentFormat::Hwp5;
    }

    // ZIP (PK\x03\x04)
    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        return DocumentFormat::Hwpx;
    }

    // UTF-8 텍스트로 시작하면 Markdown 후보
    if std::str::from_utf8(bytes).is_ok() {
        return DocumentFormat::Markdown;
    }

    DocumentFormat::Unknown
}
```

## 포맷 독립 문서 처리

Core DOM을 활용하면 입력 포맷에 관계없이 동일한 코드로 문서를 처리할 수 있습니다.

### 현재 지원되는 파이프라인

```rust,no_run
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder, HwpxRegistryBridge};
use hwpforge::md::{MdDecoder, MdDocument, MdEncoder};
use hwpforge::core::{Document, Draft, ImageStore};

// === 1. HWPX → Core DOM ===
let hwpx_result = HwpxDecoder::decode_file("input.hwpx").unwrap();
let doc_from_hwpx: Document<Draft> = hwpx_result.document;

// === 2. Markdown → Core DOM ===
let markdown = "# 제목\n\n본문 내용입니다.";
let MdDocument { document: doc_from_md, style_registry } =
    MdDecoder::decode_with_default(markdown).unwrap();

// === 3. 포맷 독립 처리 (어느 소스에서 왔든 동일) ===
fn process_document(doc: &Document<Draft>) {
    // 메타데이터 접근
    let meta = doc.metadata();
    println!("제목: {:?}", meta.title);
    println!("작성자: {:?}", meta.author);

    // 섹션/문단 순회
    for section in doc.sections() {
        println!("문단 수: {}", section.paragraphs.len());
        let counts = section.content_counts();
        println!("표: {}, 이미지: {}", counts.tables, counts.images);
    }
}

process_document(&doc_from_hwpx);
process_document(&doc_from_md);

// === 4. Core DOM → 다른 포맷으로 출력 ===
// HWPX로 저장
let bridge = HwpxRegistryBridge::from_registry(&style_registry).unwrap();
let rebound = bridge.rebind_draft_document(doc_from_md).unwrap();
let validated = rebound.validate().unwrap();
let bytes = HwpxEncoder::encode(&validated, bridge.style_store(), &ImageStore::new()).unwrap();
std::fs::write("output.hwpx", &bytes).unwrap();

// Markdown으로 저장
let markdown_out = MdEncoder::encode_lossy(&validated).unwrap();
std::fs::write("output.md", &markdown_out).unwrap();
```

### 미래: HWP5 포함 통합 파이프라인 (v2.0)

```rust,no_run,ignore
// v2.0에서 추가될 HWP5 디코더 (예시)
// use hwpforge::hwp5::Hwp5Decoder;
//
// let hwp5_result = Hwp5Decoder::decode_file("legacy.hwp")?;
// let doc: Document<Draft> = hwp5_result.document;
//
// // Core DOM을 경유하여 HWP5 → HWPX 변환
// let validated = doc.validate()?;
// let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;
// std::fs::write("converted.hwpx", &bytes)?;
```

## CLI에서 포맷 처리

현재 CLI는 HWPX와 Markdown을 지원합니다.

```bash
# Markdown → HWPX 변환
hwpforge convert report.md -o report.hwpx

# HWPX 문서 검사 (메타데이터 포함)
hwpforge inspect report.hwpx --json

# HWPX → JSON → 편집 → HWPX 라운드트립
hwpforge to-json report.hwpx -o report.json
# (AI 에이전트가 JSON 편집)
hwpforge from-json report.json -o updated.hwpx

# HWPX → Markdown (읽기용)
# Rust API: MdEncoder::encode_lossy(&validated)
```

## 크레이트 역할 분담

| 크레이트               | 역할                              | 포맷 의존성       |
| ---------------------- | --------------------------------- | ----------------- |
| `hwpforge-foundation`  | 원시 타입 (HwpUnit, Color, Index) | 없음              |
| `hwpforge-core`        | 포맷 독립 문서 모델 (IR)          | 없음              |
| `hwpforge-blueprint`   | YAML 스타일 템플릿                | 없음              |
| `hwpforge-smithy-hwpx` | HWPX ↔ Core 코덱                  | HWPX (ZIP+XML)    |
| `hwpforge-smithy-hwp5` | HWP5 → Core 디코더 (v2.0 예정)    | HWP5 (OLE/CFB)    |
| `hwpforge-smithy-md`   | Markdown ↔ Core 코덱              | Markdown (텍스트) |

**핵심 원칙**: Core 이하 계층은 어떤 파일 포맷도 모릅니다. Smithy 계층만 특정 포맷을 이해합니다.

## HWP5 포맷 구조 (참고)

HWP5 파일은 OLE2 Compound File Binary (CFB) 컨테이너 안에 바이너리 레코드 스트림을 저장합니다.

```text
HWP5 파일 (OLE2 CFB)
├── FileHeader          — 파일 인식 정보, 버전, 플래그
├── DocInfo             — 문서 설정 (스타일, 폰트, 탭, 번호)
├── BodyText/
│   ├── Section0        — 첫 번째 섹션 (바이너리 레코드)
│   ├── Section1        — 두 번째 섹션
│   └── ...
├── BinData/            — 이미지 등 바이너리 데이터
├── DocOptions/         — 추가 옵션
├── Scripts/            — 매크로 스크립트
└── PrvText             — 미리보기 텍스트
```

각 섹션은 **Tag-Length-Value (TLV)** 구조의 레코드 체인으로 구성됩니다:

```text
레코드 = TagID (10bit) + Level (10bit) + Size (12bit) + Data (Size bytes)
```

> **주의**: HWP5의 TagID에는 +16 오프셋이 있습니다. `PARA_HEADER` = 0x42 (66), 공식 스펙의 0x32 (50)가 아닙니다.

## 현재 지원 상태

| 기능                | HWPX                 | HWP5         | Markdown            |
| ------------------- | -------------------- | ------------ | ------------------- |
| **읽기 (Decode)**   | ✅ 완전 지원         | 📋 v2.0 예정 | ✅ 완전 지원        |
| **쓰기 (Encode)**   | ✅ 완전 지원         | —            | ✅ 완전 지원        |
| **메타데이터 추출** | ✅ Core DOM          | 📋 v2.0 예정 | ✅ YAML Frontmatter |
| **이미지 추출**     | ✅ ImageStore        | 📋 v2.0 예정 | —                   |
| **스타일 보존**     | ✅ HwpxStyleStore    | 📋 v2.0 예정 | ✅ StyleRegistry    |
| **JSON 라운드트립** | ✅ to-json/from-json | —            | —                   |

HWP5 읽기 지원은 v2.0 (Phase 10)에서 `hwpforge-smithy-hwp5` 크레이트로 구현될 예정입니다. Core DOM이 포맷 독립적이므로, HWP5 디코더가 추가되면 기존의 모든 Core/Blueprint/HWPX/Markdown 코드가 그대로 동작합니다.
