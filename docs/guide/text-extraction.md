# 텍스트 추출 (Text Extraction)

HwpForge의 Core DOM을 활용하여 문서에서 텍스트를 추출하고, 문서 구조(섹션, 문단, 표, 각주 등)를 보존하는 방법을 설명합니다.

> **포맷 지원 현황**: 현재 HWPX(`.hwpx`)와 Markdown(`.md`)는 이 가이드의 예제대로 바로 텍스트 추출할 수 있습니다. 레거시 HWP5(`.hwp`)는 전용 crate/CLI 경로가 이미 존재하지만, top-level guide는 아직 HWPX/Markdown 중심으로 설명합니다. 자세한 내용은 [이중 포맷 파이프라인](./format-pipeline.md)을 참고하세요.

## 문서 구조 개요

HwpForge 문서는 다음과 같은 트리 구조를 가집니다:

```text
Document
├── Metadata (title, author, created, ...)
├── Section 0
│   ├── PageSettings (용지 크기, 여백)
│   ├── Header / Footer / PageNumber (선택)
│   ├── Paragraph 0
│   │   ├── para_shape (문단 스타일 인덱스)
│   │   └── Run[]
│   │       ├── Run { content: Text("본문 텍스트"), char_shape }
│   │       ├── Run { content: Table(...), char_shape }
│   │       ├── Run { content: Image(...), char_shape }
│   │       └── Run { content: Control(Footnote/TextBox/...), char_shape }
│   ├── Paragraph 1
│   │   └── ...
│   └── ...
├── Section 1
│   └── ...
└── ...
```

핵심 타입:

| 타입                                | 설명                                       |
| ----------------------------------- | ------------------------------------------ |
| `RunContent::Text(String)`          | 일반 텍스트                                |
| `RunContent::Table(Box<Table>)`     | 인라인 표                                  |
| `RunContent::Image(Image)`          | 인라인 이미지                              |
| `RunContent::Control(Box<Control>)` | 컨트롤 (글상자, 하이퍼링크, 각주, 도형 등) |

## 기본 텍스트 추출

가장 간단한 패턴: 모든 섹션의 모든 문단에서 텍스트만 추출합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::core::run::RunContent;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
let doc = &result.document;

for section in doc.sections() {
    for paragraph in &section.paragraphs {
        for run in &paragraph.runs {
            if let RunContent::Text(ref text) = run.content {
                print!("{}", text);
            }
        }
        println!(); // 문단 끝 줄바꿈
    }
}
```

## 구조 보존 텍스트 추출

문서 구조(섹션, 문단, 표, 각주 등)를 보존하면서 텍스트를 추출합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::core::run::RunContent;
use hwpforge::core::control::Control;
use hwpforge::core::paragraph::Paragraph;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
let doc = &result.document;

// 메타데이터 출력
let meta = doc.metadata();
if let Some(title) = &meta.title {
    println!("=== {} ===", title);
}

for (sec_idx, section) in doc.sections().iter().enumerate() {
    println!("\n--- 섹션 {} ---", sec_idx + 1);

    // 머리글 텍스트
    if let Some(header) = &section.header {
        print!("[머리글] ");
        extract_paragraphs(&header.paragraphs);
    }

    // 본문 문단
    for paragraph in &section.paragraphs {
        extract_paragraph(paragraph, 0);
    }

    // 바닥글 텍스트
    if let Some(footer) = &section.footer {
        print!("[바닥글] ");
        extract_paragraphs(&footer.paragraphs);
    }
}

/// 단일 문단에서 텍스트 추출 (들여쓰기 레벨 지원)
fn extract_paragraph(para: &Paragraph, indent: usize) {
    let prefix = "  ".repeat(indent);
    print!("{}", prefix);

    for run in &para.runs {
        match &run.content {
            RunContent::Text(text) => print!("{}", text),
            RunContent::Table(table) => {
                println!("\n{}[표 {}x{}]", prefix, table.row_count(), table.col_count());
                for (r, row) in table.rows.iter().enumerate() {
                    for (c, cell) in row.cells.iter().enumerate() {
                        print!("{}  [{},{}] ", prefix, r, c);
                        extract_paragraphs(&cell.paragraphs);
                    }
                }
            }
            RunContent::Image(img) => {
                print!("[이미지: {}]", img.source_path);
            }
            RunContent::Control(ctrl) => {
                extract_control(ctrl, indent);
            }
        }
    }
    println!();
}

/// 컨트롤 요소에서 텍스트 추출
fn extract_control(ctrl: &Control, indent: usize) {
    match ctrl.as_ref() {
        Control::TextBox { paragraphs, .. } => {
            print!("[글상자] ");
            extract_paragraphs(paragraphs);
        }
        Control::Hyperlink { text, url, .. } => {
            print!("[링크: {} → {}]", text, url);
        }
        Control::Footnote { paragraphs, .. } => {
            print!("[각주: ");
            extract_paragraphs(paragraphs);
            print!("]");
        }
        Control::Endnote { paragraphs, .. } => {
            print!("[미주: ");
            extract_paragraphs(paragraphs);
            print!("]");
        }
        // 도형 (Line, Ellipse, Polygon 등)은 텍스트 없음 — 건너뜀
        _ => {}
    }
}

/// 문단 목록에서 텍스트 추출 (헬퍼)
fn extract_paragraphs(paragraphs: &[Paragraph]) {
    for para in paragraphs {
        for run in &para.runs {
            if let RunContent::Text(ref text) = run.content {
                print!("{}", text);
            }
        }
    }
}
```

## 콘텐츠 요약 (빠른 분석)

문서의 구조적 특성을 빠르게 파악하려면 `content_counts()`를 사용합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();

for (i, section) in result.document.sections().iter().enumerate() {
    let counts = section.content_counts();
    println!(
        "섹션 {}: {} 문단, {} 표, {} 이미지, {} 차트",
        i, section.paragraphs.len(), counts.tables, counts.images, counts.charts
    );
    println!(
        "  머리글={} 바닥글={} 쪽번호={}",
        section.header.is_some(),
        section.footer.is_some(),
        section.page_number.is_some()
    );
}
```

## CLI로 텍스트 추출

### inspect — 구조 요약

```bash
hwpforge inspect document.hwpx --json
```

### to-json — 전체 DOM을 JSON으로 내보내기

JSON 출력에는 모든 텍스트와 구조 정보가 포함됩니다.

```bash
hwpforge to-json document.hwpx -o doc.json
```

### Markdown 변환 — 읽기 쉬운 텍스트 추출

Rust API를 통해 HWPX를 Markdown으로 변환하면 구조를 보존한 텍스트를 얻을 수 있습니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
let validated = result.document.validate().unwrap();

// 사람이 읽기 좋은 GFM (헤딩, 표, 목록 구조 보존)
let markdown = MdEncoder::encode_lossy(&validated).unwrap();
println!("{}", markdown);
```

이 방법은 문서 구조(헤딩 계층, 표, 목록, 인용 등)를 Markdown 형식으로 자연스럽게 보존합니다.

## 레거시 HWP5 파일 처리

레거시 HWP5(`.hwp`) 파일은 현재도 다룰 수 있습니다. 다만 public guide의 중심 경로는 아직 HWPX/Markdown 쪽입니다.

현재 선택지는 이렇습니다.

1. **CLI workflow 사용**: `convert-hwp5`, `audit-hwp5`, `census-hwp5`
2. **전용 crate 사용**: `hwpforge-smithy-hwp5`의 `Hwp5Decoder`
3. **HWPX로 재출력 후 기존 guide 재사용**: 변환 결과를 HWPX guide와 같은 방식으로 처리

전용 crate 경로 예시는 다음과 같습니다.

```rust,no_run
use hwpforge_smithy_hwp5::Hwp5Decoder;
use hwpforge_core::run::RunContent;

let result = Hwp5Decoder::decode_file("legacy.hwp").unwrap();
let doc = &result.document;

for section in doc.sections() {
    for paragraph in &section.paragraphs {
        for run in &paragraph.runs {
            if let RunContent::Text(ref text) = run.content {
                print!("{}", text);
            }
        }
        println!();
    }
}
```

주의:

- HWP5 경로는 warning-first가 기본입니다.
- visual parity나 layout fidelity는 HWPX path보다 더 까다롭습니다.
- stable top-level facade는 여전히 HWPX/Markdown 중심이므로, HWP5는 전용 crate 또는 CLI를 우선 보십시오.
