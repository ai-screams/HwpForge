# Markdown에서 HWPX로

HwpForge는 Markdown을 HWPX로 변환하는 완전한 파이프라인을 제공합니다. LLM이 Markdown을 생성하면 HwpForge가 이를 한글 문서로 자동 변환합니다.

## MD → Core → HWPX 파이프라인

```text
Markdown 문자열
    |
    v (MdDecoder::decode)
Document<Draft> + StyleRegistry
    |
    v (doc.validate())
Document<Validated>
    |
    v (HwpxEncoder::encode)
HWPX 바이트 → .hwpx 파일
```

각 단계는 독립적이므로, 중간 Core DOM을 직접 조작하거나 검사할 수 있습니다.

## MdDecoder::decode() 사용법

```rust,no_run
use hwpforge_smithy_md::{MdDecoder, MdDocument};

let markdown = r#"
---
title: 사업 제안서
author: 홍길동
date: 2026-03-06
---

# 개요

본 제안서는 신규 사업 기회를 설명합니다.

## 배경

시장 분석에 따르면 성장 가능성이 높습니다.
"#;

let MdDocument { document, registry } = MdDecoder::decode(markdown).unwrap();

println!("섹션 수: {}", document.sections().len());
```

`MdDocument`에는 `document: Document<Draft>`와 `registry: StyleRegistry`가 포함됩니다.

## YAML Frontmatter

Markdown 파일 상단에 `---` 블록으로 문서 메타데이터를 지정합니다.

```yaml
---
title: 문서 제목          # Metadata.title
author: 작성자 이름        # Metadata.author
date: 2026-03-06          # Metadata.date (ISO 8601)
template: government      # 사용할 스타일 템플릿 이름 (옵션)
---
```

지원 필드:

| 필드       | 설명               |
| ---------- | ------------------ |
| `title`    | 문서 제목          |
| `author`   | 작성자             |
| `date`     | 작성일 (ISO 8601)  |
| `template` | 스타일 템플릿 이름 |

Frontmatter 없이도 디코딩이 가능하며, 메타데이터 필드는 빈 값으로 처리됩니다.

## 섹션 마커

`<!-- hwpforge:section -->` 주석으로 HWPX 섹션을 분리합니다. 한 Markdown 파일에서 여러 섹션(페이지 설정이 다른 구역)을 만들 때 유용합니다.

```markdown
# 1장 개요

첫 번째 섹션 내용.

<!-- hwpforge:section -->

# 2장 본론

두 번째 섹션 — 다른 페이지 설정 가능.
```

## H1-H6 → 개요 1-6 자동 매핑

Markdown 헤딩은 한글의 개요 스타일로 자동 변환됩니다.

| Markdown    | 한글 스타일         |
| ----------- | ------------------- |
| `# H1`      | 개요 1 (style ID 2) |
| `## H2`     | 개요 2 (style ID 3) |
| `### H3`    | 개요 3 (style ID 4) |
| `#### H4`   | 개요 4 (style ID 5) |
| `##### H5`  | 개요 5 (style ID 6) |
| `###### H6` | 개요 6 (style ID 7) |
| 일반 문단   | 본문 (style ID 0)   |

## MdEncoder — Core → Markdown

반대 방향(HWPX → Markdown) 변환도 지원합니다. 두 가지 모드가 있습니다.

```rust,no_run
use hwpforge_smithy_md::MdEncoder;
use hwpforge_smithy_hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();
let validated = result.document.validate().unwrap();

// Lossy 모드: 읽기 좋은 GFM (표, 이미지 등 일부 정보 손실)
let gfm = MdEncoder::encode_lossy(&validated).unwrap();

// Lossless 모드: YAML frontmatter + HTML-like 마크업 (정보 보존)
let lossless = MdEncoder::encode_lossless(&validated).unwrap();
```

| 모드              | 특징           | 용도                      |
| ----------------- | -------------- | ------------------------- |
| `encode_lossy`    | 읽기 좋은 GFM  | 사람이 읽는 문서 미리보기 |
| `encode_lossless` | 구조 완전 보존 | 라운드트립, 백업          |

## 전체 파이프라인 예제 (MD string → HWPX file)

```rust,no_run
use hwpforge_smithy_md::{MdDecoder, MdDocument};
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};

fn markdown_to_hwpx(markdown: &str, output_path: &str) {
    // 1. Markdown 파싱 → Core DOM
    let MdDocument { document, registry: _ } = MdDecoder::decode(markdown).unwrap();

    // 2. 문서 검증
    let validated = document.validate().unwrap();

    // 3. 한컴 기본 스타일 적용 후 HWPX 인코딩
    let style_store = HwpxStyleStore::default_modern();
    let image_store = Default::default();
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();

    // 4. 파일 저장
    std::fs::write(output_path, &bytes).unwrap();
    println!("저장 완료: {output_path}");
}

fn main() {
    let md = r#"
---
title: AI 활용 정책 제안서
author: 정책팀
date: 2026-03-06
---

# 제안 배경

인공지능 기술의 급속한 발전에 대응하여 정책 수립이 필요합니다.

## 현황 분석

국내외 AI 활용 사례를 분석하였습니다.

## 정책 방향

단계적 도입과 윤리적 기준 마련을 제안합니다.
"#;

    markdown_to_hwpx(md, "proposal.hwpx");
}
```
