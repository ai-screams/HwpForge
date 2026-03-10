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
use hwpforge::md::{MdDecoder, MdDocument};

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

let MdDocument { document, style_registry } = MdDecoder::decode_with_default(markdown).unwrap();

println!("섹션 수: {}", document.sections().len());
```

`MdDocument`에는 `document: Document<Draft>`와 `style_registry: StyleRegistry`가 포함됩니다.

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

| 필드       | Metadata 필드 | 설명                      |
| ---------- | ------------- | ------------------------- |
| `title`    | `title`       | 문서 제목                 |
| `author`   | `author`      | 작성자                    |
| `date`     | `created`     | 작성일 (ISO 8601)         |
| `subject`  | `subject`     | 주제/설명                 |
| `keywords` | `keywords`    | 검색 키워드 (YAML 배열)   |
| `modified` | `modified`    | 수정일 (ISO 8601)         |
| `template` | _(스타일)_    | 스타일 템플릿 이름 (옵션) |

Frontmatter 없이도 디코딩이 가능하며, 메타데이터 필드는 빈 값으로 처리됩니다.

### 디코딩 후 메타데이터 확인

```rust,no_run
use hwpforge::md::{MdDecoder, MdDocument};

let markdown = "---\ntitle: 보고서\nauthor: 홍길동\ndate: 2026-03-06\n---\n\n# 본문\n";
let MdDocument { document, .. } = MdDecoder::decode_with_default(markdown).unwrap();

let meta = document.metadata();
assert_eq!(meta.title.as_deref(), Some("보고서"));
assert_eq!(meta.author.as_deref(), Some("홍길동"));
assert_eq!(meta.created.as_deref(), Some("2026-03-06"));
```

전체 메타데이터 필드와 프로그래밍 설정 방법은 [메타데이터 가이드](./metadata.md)를 참고하세요.

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
use hwpforge::md::MdEncoder;
use hwpforge::hwpx::HwpxDecoder;

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
use hwpforge::md::{MdDecoder, MdDocument};
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};

fn markdown_to_hwpx(markdown: &str, output_path: &str) {
    // 1. Markdown 파싱 → Core DOM
    let MdDocument { document, .. } = MdDecoder::decode_with_default(markdown).unwrap();

    // 2. 문서 검증
    let validated = document.validate().unwrap();

    // 3. 한컴 기본 스타일 적용 후 HWPX 인코딩
    let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
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

## HWPX → Markdown 변환 (RAG/LLM 활용)

HWPX 문서를 Markdown으로 변환하면 LLM이나 RAG(Retrieval-Augmented Generation) 시스템에서 직접 활용할 수 있습니다.

### 의존성 설정

`Cargo.toml`에 `md` 기능을 활성화합니다:

```toml
[dependencies]
hwpforge = { version = "0.1", features = ["md"] }
```

### Lossy vs Lossless 모드 선택

| 기준          | Lossy (`encode_lossy`)      | Lossless (`encode_lossless`)   |
| ------------- | --------------------------- | ------------------------------ |
| **출력 형식** | 표준 GFM Markdown           | YAML frontmatter + HTML 마크업 |
| **가독성**    | 높음 (사람/LLM 모두)        | 낮음 (기계 파싱용)             |
| **정보 손실** | 스타일/레이아웃 일부 손실   | 구조 완전 보존                 |
| **RAG 추천**  | **추천** — 청크 분할에 적합 | 원본 복원이 필요할 때만        |
| **LLM 추천**  | **추천** — 토큰 효율적      | 라운드트립 편집 시             |

**RAG 시스템에서는 `encode_lossy`를 권장합니다.** 표준 GFM으로 출력되어 청크 분할기(text splitter)와 호환성이 높고, 불필요한 마크업이 없어 토큰을 절약합니다.

### 완전한 HWPX → Markdown 예제 (에러 처리 포함)

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;
use std::path::Path;

fn hwpx_to_markdown(input_path: &str) -> Result<String, Box<dyn std::error::Error>> {
    // 1. 파일 존재 여부 확인
    let path = Path::new(input_path);
    if !path.exists() {
        return Err(format!("파일을 찾을 수 없습니다: {}", input_path).into());
    }

    // 2. HWPX 디코딩
    let result = HwpxDecoder::decode_file(input_path)
        .map_err(|e| format!("HWPX 디코딩 실패: {e}"))?;

    // 3. 메타데이터 확인 (선택)
    let meta = result.document.metadata();
    if let Some(title) = &meta.title {
        eprintln!("문서 제목: {}", title);
    }

    // 4. Draft → Validated 상태 전이
    let validated = result.document.validate()
        .map_err(|e| format!("문서 검증 실패: {e}"))?;

    // 5. Markdown 변환 (RAG용 lossy 모드)
    let markdown = MdEncoder::encode_lossy(&validated)
        .map_err(|e| format!("Markdown 인코딩 실패: {e}"))?;

    Ok(markdown)
}

fn main() {
    match hwpx_to_markdown("document.hwpx") {
        Ok(md) => {
            std::fs::write("output.md", &md).expect("파일 저장 실패");
            println!("변환 완료: {} bytes", md.len());
        }
        Err(e) => eprintln!("오류: {e}"),
    }
}
```

### 대량 파일 변환

여러 HWPX 파일을 Markdown으로 일괄 변환합니다:

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::md::MdEncoder;
use std::path::Path;

fn batch_convert(input_dir: &str, output_dir: &str) -> Result<usize, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(output_dir)?;
    let mut count = 0;

    for entry in std::fs::read_dir(input_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().is_some_and(|ext| ext == "hwpx") {
            let result = HwpxDecoder::decode_file(&path)?;
            let validated = result.document.validate()?;
            let markdown = MdEncoder::encode_lossy(&validated)?;

            let out_name = path.file_stem().unwrap().to_string_lossy();
            let out_path = Path::new(output_dir).join(format!("{}.md", out_name));
            std::fs::write(&out_path, &markdown)?;

            eprintln!("변환: {} → {}", path.display(), out_path.display());
            count += 1;
        }
    }

    Ok(count)
}
```

### CLI로 변환

```bash
# Markdown → HWPX
hwpforge convert report.md -o report.hwpx

# HWPX 구조 확인 후 JSON으로 추출 (Markdown 변환 대안)
hwpforge inspect document.hwpx --json
hwpforge to-json document.hwpx -o document.json
```

> **참고**: CLI의 `convert` 명령은 현재 Markdown → HWPX 방향만 지원합니다. HWPX → Markdown 변환은 Rust API(`MdEncoder`)를 사용하세요.
