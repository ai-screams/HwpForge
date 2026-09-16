# 메타데이터 (Metadata)

HwpForge의 모든 문서는 `Metadata` 구조체를 통해 제목, 작성자, 작성일 등의 메타데이터를 관리합니다.

## Metadata 구조체

```rust
#[non_exhaustive]
pub struct Metadata {
    pub title: Option<String>,               // 문서 제목
    pub author: Option<String>,              // 작성자
    pub subject: Option<String>,             // 주제/설명
    pub description: Option<String>,         // 자유 서술 요약 (subject와 별개)
    pub last_saved_by: Option<String>,       // 마지막으로 저장한 사람 (author와 별개)
    pub keywords: Vec<String>,               // 검색 키워드
    pub created: Option<String>,             // 작성일 (ISO 8601, 예: "2026-03-06")
    pub modified: Option<String>,            // 수정일 (ISO 8601)
    pub extras: BTreeMap<String, String>,    // 아직 타입 필드로 승격되지 않은 <opf:meta> 등 원본 항목
}
```

모든 필드는 선택적입니다. `Metadata::default()`는 모든 필드가 비어 있는 상태를 반환합니다.

`Metadata`는 `#[non_exhaustive]`입니다 — 향후 버전에서 필드가 추가될 수 있으므로, 구조체 리터럴로 생성할 때는 반드시 `..Default::default()` (또는 `..Metadata::default()`)를 함께 사용해야 합니다.

## 기존 HWPX 파일에서 메타데이터 읽기

`HwpxDecoder`로 HWPX 파일을 디코딩한 후 `document.metadata()`로 접근합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;

fn main() -> anyhow::Result<()> {
    let result = HwpxDecoder::decode_file("document.hwpx")?;
    let meta = result.document.metadata();

    // 개별 필드 접근
    if let Some(title) = &meta.title {
        println!("제목: {}", title);
    }
    if let Some(author) = &meta.author {
        println!("작성자: {}", author);
    }
    if let Some(created) = &meta.created {
        println!("작성일: {}", created);
    }
    if let Some(subject) = &meta.subject {
        println!("주제: {}", subject);
    }
    if !meta.keywords.is_empty() {
        println!("키워드: {}", meta.keywords.join(", "));
    }

    Ok(())
}
```

## Markdown에서 메타데이터 설정

YAML Frontmatter로 메타데이터를 지정하면 `MdDecoder`가 자동으로 `Metadata` 필드에 매핑합니다.

```rust,no_run
use hwpforge::md::{MdDecoder, MdDocument};

let markdown = r#"---
title: 분기 보고서
author: 김철수
date: 2026-03-06
metadata:
  subject: 2026년 1분기 경영실적 보고
  keywords:
    - 분기실적
    - 경영보고
  modified: 2026-03-10
---

# 보고서 본문

내용이 여기에 들어갑니다.
"#;

let MdDocument { document, style_registry } = MdDecoder::decode_with_default(markdown).unwrap();

let meta = document.metadata();
assert_eq!(meta.title.as_deref(), Some("분기 보고서"));
assert_eq!(meta.author.as_deref(), Some("김철수"));
assert_eq!(meta.created.as_deref(), Some("2026-03-06"));
assert_eq!(meta.subject.as_deref(), Some("2026년 1분기 경영실적 보고"));
assert_eq!(meta.keywords, vec!["분기실적", "경영보고"]);
assert_eq!(meta.modified.as_deref(), Some("2026-03-10"));
```

### Frontmatter 필드 매핑

최상위 필드:

| YAML 필드  | Metadata 필드 | 설명                                              |
| ---------- | ------------- | ------------------------------------------------- |
| `title`    | `title`       | 문서 제목                                         |
| `author`   | `author`      | 작성자                                            |
| `date`     | `created`     | 작성일 (ISO 8601)                                 |
| `template` | _(없음)_      | 파싱되지만 스타일 선택에는 쓰이지 않습니다(inert) |
| `metadata` | _(중첩 맵)_   | 아래 하위 필드를 담는 컨테이너                    |

`metadata:` 아래의 하위 필드:

| 하위 필드  | Metadata 필드 | 설명               |
| ---------- | ------------- | ------------------ |
| `subject`  | `subject`     | 주제/설명          |
| `keywords` | `keywords`    | 검색 키워드 (배열) |
| `modified` | `modified`    | 수정일 (ISO 8601)  |

`subject`/`keywords`/`modified`는 반드시 `metadata:` 아래에 중첩해야 합니다 — 최상위에 쓰면 조용히 무시됩니다.

## 프로그래밍으로 메타데이터 설정

`Document<Draft>` 상태에서 `metadata_mut()`으로 직접 설정할 수 있습니다.

```rust,no_run
use hwpforge::core::{Document, Draft, Metadata, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};

let mut doc = Document::<Draft>::new();

// 메타데이터 설정
doc.metadata_mut().title = Some("제안서".to_string());
doc.metadata_mut().author = Some("홍길동".to_string());
doc.metadata_mut().created = Some("2026-03-06".to_string());
doc.metadata_mut().subject = Some("신규 사업 제안".to_string());
doc.metadata_mut().keywords = vec!["사업".to_string(), "제안".to_string()];

// 또는 Metadata 구조체를 직접 생성하여 설정
let meta = Metadata {
    title: Some("제안서".to_string()),
    author: Some("홍길동".to_string()),
    created: Some("2026-03-06".to_string()),
    ..Metadata::default()
};
doc.set_metadata(meta);

// 섹션 추가 후 검증/인코딩
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::text("본문 내용", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));
let validated = doc.validate().unwrap();
```

## CLI에서 메타데이터 확인

`hwpforge inspect` 명령으로 HWPX 파일의 메타데이터를 확인합니다.

```bash
# 사람이 읽기 좋은 출력
hwpforge inspect document.hwpx

# 출력 예시:
# Document: document.hwpx
#   Title:  분기 보고서
#   Author: 김철수
#   Sections: 1
#     [0] 5 paras, 1 tables, 0 images, 0 charts | header=false footer=false pagenum=false
```

```bash
# JSON 출력 (AI 에이전트용)
hwpforge inspect document.hwpx --json

# 출력 예시:
# {
#   "status": "ok",
#   "metadata": {
#     "title": "분기 보고서",
#     "author": "김철수"
#   },
#   "sections": [...]
# }
```

## JSON 라운드트립에서 메타데이터

`to-json`으로 내보내면 메타데이터가 JSON에 포함됩니다.

```bash
hwpforge to-json document.hwpx -o doc.json
```

```json
{
  "document": {
    "sections": [...],
    "metadata": {
      "title": "분기 보고서",
      "author": "김철수",
      "subject": null,
      "keywords": [],
      "created": "2026-03-06",
      "modified": null
    }
  },
  "styles": {...}
}
```

AI 에이전트가 JSON에서 메타데이터를 수정한 후 `from-json`으로 HWPX를 재생성할 수 있습니다.

```bash
# JSON 편집 후 HWPX로 변환
hwpforge from-json doc.json -o updated.hwpx
```

## MCP 도구에서 메타데이터 확인

`hwpforge_inspect` MCP 도구로 메타데이터를 포함한 문서 구조를 확인합니다.

```json
{
  "tool": "hwpforge_inspect",
  "arguments": {
    "file_path": "/path/to/document.hwpx"
  }
}
```

## 현재 제한사항

- **HWPX 네이티브 메타데이터**: 한글 프로그램으로 작성된 HWPX 파일의 `META-INF/` 내 네이티브 메타데이터 추출은 아직 지원하지 않습니다. Markdown Frontmatter로 설정된 메타데이터와 `to-json`/`from-json` 라운드트립을 통한 메타데이터만 보존됩니다.
- **타임스탬프 형식**: `created`/`modified`는 `Option<String>` (ISO 8601 문자열)입니다. `chrono` 등 날짜 라이브러리와 연동 시 직접 파싱이 필요합니다.
