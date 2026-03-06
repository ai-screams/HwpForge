# 스타일 템플릿 (YAML)

## Blueprint 개념: 구조와 스타일 분리

HwpForge는 HTML+CSS와 동일한 철학으로 **구조(Core)**와 **스타일(Blueprint)**을 분리합니다.

```text
Core (Document, Section, Paragraph, Run)
    = HTML — "무엇이 있는가"

Blueprint (Template, StyleRegistry, CharShape, ParaShape)
    = CSS  — "어떻게 보이는가"
```

Core의 문단과 런은 스타일 인덱스(`ParaShapeIndex`, `CharShapeIndex`)만 참조합니다. 실제 폰트 이름이나 크기는 Blueprint에 정의됩니다. 덕분에 동일한 문서 구조에 다른 템플릿을 적용해 전혀 다른 외관의 HWPX를 생성할 수 있습니다.

## Template YAML 구조

```yaml
meta:
  name: my-template
  version: "1.0"
  description: "커스텀 스타일 템플릿"

styles:
  body:
    font: "한컴바탕"
    size: 10pt
    line_spacing: 160%
    alignment: justify

  heading1:
    inherits: body        # body에서 상속
    font: "한컴고딕"
    size: 16pt
    bold: true

  heading2:
    inherits: heading1
    size: 14pt
```

### 상속 (Inheritance)

`inherits` 키로 다른 스타일을 상속받습니다. 상속 체인은 DFS로 해결되며, 자식 스타일의 값이 부모를 덮어씁니다. `Option` 필드(`PartialCharShape`, `PartialParaShape`)를 병합하는 two-type 패턴으로 구현됩니다.

## StyleRegistry: from_template() 사용법

```rust,no_run
use hwpforge_blueprint::template::Template;
use hwpforge_blueprint::registry::StyleRegistry;

let yaml = r#"
meta:
  name: custom
  version: "1.0"
styles:
  body:
    font: "나눔명조"
    size: 11pt
"#;

// YAML → Template → StyleRegistry
let template = Template::from_yaml(yaml).unwrap();
let registry = StyleRegistry::from_template(&template).unwrap();

// 인덱스 기반 접근 (브랜드 타입으로 혼용 방지)
let body_entry = registry.get_style("body").unwrap();
let char_shape = registry.char_shape(body_entry.char_shape_id).unwrap();
println!("폰트: {}", char_shape.font);       // "나눔명조"
println!("크기: {:?}", char_shape.size);     // HwpUnit
```

## 내장 템플릿: builtin_default()

별도 YAML 없이 즉시 사용 가능한 기본 템플릿입니다.

```rust,no_run
use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_blueprint::registry::StyleRegistry;

let template = builtin_default().unwrap();
assert_eq!(template.meta.name, "default");

let registry = StyleRegistry::from_template(&template).unwrap();
let body = registry.get_style("body").unwrap();
let cs = registry.char_shape(body.char_shape_id).unwrap();
assert_eq!(cs.font, "한컴바탕");
```

## HwpxStyleStore 변환: from_registry()

Blueprint의 `StyleRegistry`를 HWPX 인코더가 사용하는 `HwpxStyleStore`로 변환합니다.

```rust,no_run
use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_smithy_hwpx::HwpxStyleStore;

let template = builtin_default().unwrap();
let registry = StyleRegistry::from_template(&template).unwrap();

// Blueprint StyleRegistry → HWPX 전용 스타일 저장소
let style_store = HwpxStyleStore::from_registry(&registry);
```

## 한컴 스타일셋: Classic / Modern / Latest

한글 프로그램은 버전에 따라 다른 기본 스타일 구성을 사용합니다.

| 스타일셋  | 스타일 수 | 설명                    |
| --------- | --------- | ----------------------- |
| `Classic` | 18개      | 한글 구버전 호환        |
| `Modern`  | 22개      | **기본값** (한글 2018~) |
| `Latest`  | 23개      | 최신 버전               |

`Modern`은 개요 8/9/10을 스타일 ID 9-11에 삽입하므로 인덱스가 Classic과 다릅니다.

```rust,no_run
use hwpforge_smithy_hwpx::{HwpxStyleStore, HancomStyleSet};

// 기본값 (간단한 방법)
let modern = HwpxStyleStore::with_default_fonts("함초롬바탕");

// 특정 스타일셋 지정
// from_registry_with()로 커스텀 레지스트리 + 스타일셋 조합 가능
```

## 예제: 커스텀 스타일로 문서 생성

```rust,no_run
use hwpforge_blueprint::template::Template;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_core::{Document, Section, Paragraph, PageSettings};
use hwpforge_core::run::Run;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

let yaml = r#"
meta:
  name: report
  version: "1.0"
styles:
  body:
    font: "맑은 고딕"
    size: 10pt
    line_spacing: 150%
  title:
    inherits: body
    font: "맑은 고딕"
    size: 20pt
    bold: true
    alignment: center
"#;

// 스타일 빌드
let template = Template::from_yaml(yaml).unwrap();
let registry = StyleRegistry::from_template(&template).unwrap();
let style_store = HwpxStyleStore::from_registry(&registry);

// 문서 구성 (스타일 인덱스는 레지스트리에서 조회)
let mut doc = Document::new();
doc.add_section(Section::with_paragraphs(
    vec![
        // 제목 문단 (ParaShapeIndex 0 = title)
        Paragraph::with_runs(
            vec![Run::text("분기 보고서", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ),
        // 본문 문단 (ParaShapeIndex 1 = body)
        Paragraph::with_runs(
            vec![Run::text("1분기 실적은 목표를 초과 달성했습니다.", CharShapeIndex::new(1))],
            ParaShapeIndex::new(1),
        ),
    ],
    PageSettings::a4(),
));

let validated = doc.validate().unwrap();
let image_store = Default::default();
let bytes = HwpxEncoder::encode(&validated, &style_store, &image_store).unwrap();
std::fs::write("report.hwpx", &bytes).unwrap();
```
