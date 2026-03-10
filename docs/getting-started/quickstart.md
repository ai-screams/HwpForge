# 빠른 시작

이 페이지에서는 HwpForge의 세 가지 핵심 사용 패턴을 코드 예제와 함께 설명합니다.

## 예제 1: 텍스트 문서 생성 후 HWPX로 저장

가장 기본적인 사용 패턴입니다. 문서 구조를 직접 조립하고 HWPX 파일로 출력합니다.

```rust,no_run
use hwpforge::core::{Document, Draft, ImageStore, PageSettings, Paragraph, Run, Section};
use hwpforge::foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};

fn main() -> anyhow::Result<()> {
    // 1. Draft 상태의 문서 생성
    let mut doc = Document::<Draft>::new();

    // 2. 텍스트 Run 구성 — CharShapeIndex(0)은 기본 글자 스타일을 참조
    let run = Run::text("안녕하세요, HwpForge입니다!", CharShapeIndex::new(0));

    // 3. 문단 생성 — ParaShapeIndex(0)은 기본 문단 스타일을 참조
    let paragraph = Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));

    // 4. 섹션(= 쪽 단위 컨테이너)에 문단 추가, A4 용지 설정
    let section = Section::with_paragraphs(vec![paragraph], PageSettings::a4());
    doc.add_section(section);

    // 5. 유효성 검증 — Draft → Validated 상태 전이 (타입스테이트)
    let validated = doc.validate()?;

    // 6. 스타일 스토어: 한컴 Modern(22종) 기본 스타일 사용
    let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");

    // 7. 이미지 스토어: 이미지가 없으므로 빈 스토어 사용
    let image_store = ImageStore::new();

    // 8. HWPX 바이트 인코딩 후 파일 저장
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;
    std::fs::write("output.hwpx", &bytes)?;

    println!("output.hwpx 저장 완료 ({} bytes)", bytes.len());
    Ok(())
}
```

> **참고**: `CharShapeIndex::new(0)`과 `ParaShapeIndex::new(0)`은 `HwpxStyleStore::with_default_fonts()`이
> 제공하는 기본 스타일(본문)을 가리킵니다. 커스텀 스타일을 사용하려면
> [스타일 템플릿](../guide/style-templates.md) 문서를 참고하세요.

---

## 예제 2: HWPX 파일 디코딩

기존 HWPX 파일을 읽어서 Core 문서 모델로 변환합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;
use hwpforge::core::run::RunContent;

fn main() -> anyhow::Result<()> {
    // 파일 경로를 받아 HWPX를 디코딩
    let result = HwpxDecoder::decode_file("input.hwpx")?;

    let doc = &result.document;

    // 섹션 수 출력
    println!("섹션 수: {}", doc.sections().len());

    // 메타데이터 접근 (제목, 작성자, 작성일 등)
    let meta = doc.metadata();
    if let Some(title) = &meta.title {
        println!("제목: {}", title);
    }
    if let Some(author) = &meta.author {
        println!("작성자: {}", author);
    }

    // 각 섹션의 문단과 텍스트 출력
    for (sec_idx, section) in doc.sections().iter().enumerate() {
        println!("--- 섹션 {} ---", sec_idx + 1);
        for paragraph in &section.paragraphs {
            for run in &paragraph.runs {
                if let RunContent::Text(ref text) = run.content {
                    print!("{}", text);
                }
            }
            println!(); // 문단 끝 줄바꿈
        }
    }

    Ok(())
}
```

`HwpxDecoder::decode_file`은 경로를 받아 ZIP을 열고 XML을 파싱합니다.
반환값에는 `document`(문서 구조), `style_store`(글꼴/문단 스타일), `image_store`(이미지)가 포함됩니다.
`document.metadata()`로 제목, 작성자 등의 메타데이터에 접근할 수 있습니다.

---

## 예제 3: Markdown → HWPX 변환

GFM(GitHub Flavored Markdown) 텍스트를 HWPX 파일로 변환합니다.
`features = ["md"]` 또는 `features = ["full"]`이 필요합니다.

```rust,no_run
use hwpforge::core::ImageStore;
use hwpforge::hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge::md::MdDecoder;

fn main() -> anyhow::Result<()> {
    // 1. GFM Markdown 텍스트 (YAML 프론트매터 지원)
    let markdown = r#"---
title: 보고서 제목
author: 홍길동
date: 2026-03-06
---

# 1장. 서론

본 보고서는 HwpForge를 이용한 **자동 문서 생성** 예시입니다.

## 1.1 배경

- 항목 A
- 항목 B
- 항목 C

## 1.2 결론

> HwpForge는 LLM 에이전트가 한글 문서를 생성할 때 사용할 수 있습니다.
"#;

    // 2. Markdown → Core 문서 모델 변환
    //    MdDecoder::decode_with_default는 document + style_registry(스타일 매핑)를 반환
    let md_doc = MdDecoder::decode_with_default(markdown)?;

    // 3. Draft → Validated 상태 전이
    let validated = md_doc.document.validate()?;

    // 4. Markdown 헤딩(H1-H6)이 한컴 개요 1-6 스타일로 자동 매핑된 스타일 스토어 생성
    let style_store = HwpxStyleStore::from_registry(&md_doc.style_registry);

    // 5. 이미지 없음
    let image_store = ImageStore::new();

    // 6. HWPX 인코딩 후 저장
    let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store)?;
    std::fs::write("report.hwpx", &bytes)?;

    println!("report.hwpx 저장 완료");
    Ok(())
}
```

Markdown 변환 시 자동으로 처리되는 항목:

| Markdown 요소        | 변환 결과              |
| -------------------- | ---------------------- |
| `# H1` ~ `###### H6` | 한컴 개요 1 ~ 6 스타일 |
| `**굵게**`           | 글자 진하게            |
| `*기울임*`           | 글자 기울임            |
| `` `코드` ``         | 고정폭 글꼴            |
| `> 인용문`           | 들여쓰기 문단          |
| `- 목록`             | 글머리 기호 목록       |
| YAML 프론트매터      | 문서 메타데이터        |

## 다음 단계

- [아키텍처 개요](./architecture.md) — 크레이트 구조와 설계 원칙 이해
- [스타일 템플릿](../guide/style-templates.md) — 커스텀 글꼴/색상 템플릿 작성
- [API 레퍼런스](https://docs.rs/hwpforge) — 전체 공개 API 문서
