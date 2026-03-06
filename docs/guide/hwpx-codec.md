# HWPX 인코딩/디코딩

## HWPX 포맷 소개

HWPX는 한글과컴퓨터의 공개 문서 표준(KS X 6101, OWPML)입니다. 내부 구조는 ZIP 컨테이너 안에 XML 파일들이 담긴 형태로, Microsoft DOCX와 유사합니다.

주요 구성 파일:

- `mimetype` — 포맷 식별자
- `Contents/header.xml` — 스타일 정의 (폰트, 문단 모양, 글자 모양)
- `Contents/section0.xml`, `section1.xml`, … — 본문 내용
- `BinData/` — 이미지 등 바이너리 파일들
- `Chart/` — 차트 XML (OOXML `xmlns:c` 형식)

`hwpforge-smithy-hwpx` 크레이트가 이 포맷의 인코드/디코드를 담당합니다.

## 디코딩: HWPX 파일 읽기

`HwpxDecoder::decode_file()`로 .hwpx 파일을 `HwpxDocument`로 읽습니다.

```rust,no_run
use hwpforge_smithy_hwpx::HwpxDecoder;

let result = HwpxDecoder::decode_file("document.hwpx").unwrap();

// 섹션 수 확인
println!("섹션 수: {}", result.document.sections().len());

// 첫 번째 섹션의 문단 수
let section = &result.document.sections()[0];
println!("문단 수: {}", section.paragraphs.len());
```

## HwpxDocument 결과 구조

`HwpxDecoder::decode_file()`은 `HwpxDocument`를 반환합니다. 세 가지 필드로 구성됩니다.

```rust,no_run
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxDocument};

let HwpxDocument { document, style_store, image_store } =
    HwpxDecoder::decode_file("document.hwpx").unwrap();

// document: Document<Draft> — 문서 DOM (섹션/문단/런 트리)
// style_store: HwpxStyleStore — 폰트, 글자 모양, 문단 모양, 스타일
// image_store: ImageStore — 임베드된 이미지 바이너리 데이터
```

| 필드          | 타입              | 설명                          |
| ------------- | ----------------- | ----------------------------- |
| `document`    | `Document<Draft>` | 섹션, 문단, 런 트리           |
| `style_store` | `HwpxStyleStore`  | 폰트/글자모양/문단모양/스타일 |
| `image_store` | `ImageStore`      | 이미지 바이너리 저장소        |

## 인코딩: Core → HWPX

`HwpxEncoder::encode()`로 `Document<Validated>`를 HWPX 바이트 벡터로 직렬화합니다.

```rust,no_run
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxStyleStore};
use hwpforge_core::{Document, Section, Paragraph, PageSettings};
use hwpforge_core::run::Run;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

// 새 문서 생성
let mut doc = Document::new();
doc.add_section(Section::with_paragraphs(
    vec![Paragraph::with_runs(
        vec![Run::text("안녕하세요, HwpForge!", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )],
    PageSettings::a4(),
));

let validated = doc.validate().unwrap();
let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
let image_store = Default::default();

let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
std::fs::write("output.hwpx", &bytes).unwrap();
```

## HwpxStyleStore 생성 방법

### with_default_fonts() — 간단한 기본 스타일

단일 글꼴 이름으로 빠르게 스타일 스토어를 생성합니다. 가장 간단한 방법입니다.

```rust,no_run
use hwpforge::hwpx::HwpxStyleStore;

let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
```

### from_registry() — Blueprint 템플릿에서 변환

커스텀 YAML 스타일 템플릿을 적용할 때 사용합니다. 자세한 내용은 [스타일 템플릿](./style-templates.md) 참조.

```rust,no_run
use hwpforge::blueprint::builtins::builtin_default;
use hwpforge::blueprint::registry::StyleRegistry;
use hwpforge::hwpx::HwpxStyleStore;

let template = builtin_default().unwrap();
let registry = StyleRegistry::from_template(&template).unwrap();
let style_store = HwpxStyleStore::from_registry(&registry);
```

## 라운드트립 예제 (decode → modify → encode)

기존 HWPX 파일을 읽어서 수정한 뒤 다시 저장하는 전형적인 패턴입니다.

```rust,no_run
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge_core::run::Run;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

// 1. 기존 파일 디코딩
let mut result = HwpxDecoder::decode_file("original.hwpx").unwrap();

// 2. 문서 수정: 새 문단 추가
let new_para = Paragraph::with_runs(
    vec![Run::text("추가된 문단입니다.", CharShapeIndex::new(0))],
    ParaShapeIndex::new(0),
);
// Draft 상태이므로 sections 직접 접근 가능
result.document.sections_mut()[0].paragraphs.push(new_para);

// 3. 검증 후 인코딩
let validated = result.document.validate().unwrap();
let bytes = HwpxEncoder::encode(
    &validated,
    &result.style_store,
    &result.image_store,
).unwrap();

std::fs::write("modified.hwpx", &bytes).unwrap();
```

## 오류 처리

모든 함수는 `HwpxResult<T>`를 반환합니다. `HwpxError`는 `HwpxErrorCode`와 메시지를 포함합니다.

```rust,no_run
use hwpforge::hwpx::HwpxDecoder;

match HwpxDecoder::decode_file("missing.hwpx") {
    Ok(_result) => println!("디코딩 성공"),
    Err(e) => eprintln!("디코딩 실패: {e}"),
}
```
