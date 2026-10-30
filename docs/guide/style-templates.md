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

### 글자 그림자 (`shadow`)

`char_shape.shadow` 는 종류만 적는 문자열이나, 종류·색·간격을 적는 맵으로 씁니다. 간격은 글자 크기 대비 % 로 −100 이상 100 이하, 색은 `"#RRGGBB"` 또는 색 없음 `none` 입니다.

```yaml
# parent.yaml
meta:
  name: parent
styles:
  heading1:
    char_shape:
      font: 한컴바탕
      size: 16pt
      shadow:                 # 맵: 종류·색·간격
        kind: continuous
        color: '#123456'
        offset_x: -3
        offset_y: 4
  body:
    char_shape:
      font: 한컴바탕
      size: 10pt
      shadow: drop            # 문자열: 종류만 (drop · continuous · none)

# child.yaml
meta:
  name: child
  extends: parent
styles:
  heading1:
    char_shape:
      shadow: { color: '#00FF00' }   # 색만 바꿈 — 종류·간격은 parent 의 것
```

`extends` 로 이어진 템플릿 사이의 상속은 칸별입니다. 자식이 `shadow: { color: '#00FF00' }` 만 적으면 부모의 종류·간격은 그대로이고 색만 바뀝니다(위 예의 child heading1 = 연속 `#00FF00` −3·4). `color: none` 은 색을 지우는 명시 값이고, 키를 적지 않으면 부모 값을 물려받습니다. 끝까지 정해지지 않은 칸은 한컴 새 문서의 값(종류 없음·`#C0C0C0`·10·10)이 됩니다 — 위 예의 body 는 비연속 `#C0C0C0` 10·10. `color: null`, 모르는 키, 범위 밖 간격은 오류입니다.

옛 템플릿의 `shadow: drop` 은 그대로 읽힙니다. 다만 색을 적지 않고 그림자를 켜면 0.17.0 부터 한컴 새 문서와 같은 `#C0C0C0` 으로 그려져, 이전(`#B2B2B2`)보다 밝게 보입니다. 이전 색이 필요하면 `shadow: {kind: drop, color: '#B2B2B2'}` 처럼 색을 적습니다.

Rust API 에서 결정값 `CharShape.shadow` 는 `ShadowType` 이 아니라 `hwpforge_foundation::CharShadow`(종류·색·간격), 부분값 `PartialCharShape.shadow` 는 `Option<PartialCharShadow>` 입니다(0.17.0 breaking).

### 다단 템플릿 상속의 우선순위

`extends` 체인은 root → parent → leaf 순서로 병합합니다. 같은 필드나 `markdown_mapping` 키를 여러 조상이 정의하면 가장 가까운 명시값이 이기며, leaf가 적은 값은 항상 우선합니다. root의 글자 크기가 12pt, parent가 20pt이고 leaf가 생략하면 결과는 20pt입니다.

이 동작은 먼 조상의 값이 먼저 채워지던 이전 다단 상속 결과를 교정합니다. 이전 결과가 필요하면 leaf에 크기와 mapping을 명시하세요. 스타일 entry도 root의 선언 순서부터 누적하므로 첫 entry를 쓰던 본문 폴백 결과가 달라질 수 있습니다. 종전 선택을 유지하려면 `markdown_mapping.body`를 명시하세요.

## Markdown 목록 역할

Markdown 목록([Markdown에서 HWPX로](./markdown-bridge.md)의 "목록 → 한컴 글머리표·번호 문단" 절)은 `markdown_mapping` 의 목록 역할이 가리키는 스타일과 템플릿의 `numberings`·`bullets` 정의로 씁니다. 내장 `default` 템플릿(과 그것을 상속하는 `gov_proposal`)의 선언은 다음과 같습니다(글자·여백 외형은 줄였습니다).

```yaml
styles:
  list_item:      # 글머리표 항목, 그리고 모든 목록의 공통 여백
    para_shape:
      indent: { left: 20pt, first_line: -10pt }
      list: { kind: bullet, bullet_id: 1, level: 0 }
  numbered_item:  # 번호 항목
    para_shape:
      list: { kind: number, numbering_id: 2, level: 0 }
  check_item:     # 체크 항목
    para_shape:
      list: { kind: check_bullet, bullet_id: 11, level: 0, checked: false }

numberings:
  - id: 1         # 첫 번호 정의 = 제목(개요)용
    start: 0
    levels: [...]  # 수준 1–10
  - id: 2         # 번호 항목용: 수준 1–10 모두 Digit, '^1.' … '^10.'
    start: 0
    levels: [...]

bullets:          # id 1–10: 수준 0–9 의 기호(• ◦ ▪ 되풀이), id 11: ☐/☑
  - { id: 1, bullet_char: '•', use_image: false, para_head: { start: 0, level: 1, num_format: Digit, text: '', checkable: false } }
  # …
  - { id: 11, bullet_char: '☐', checked_char: '☑', use_image: false, para_head: { start: 0, level: 1, num_format: Digit, text: '', checkable: true } }

markdown_mapping:
  list_item: list_item
  numbered_item: numbered_item
  check_item: check_item
  bullet_levels: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
```

| 키              | 뜻                                                                                                                                                                                                                                               |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `list_item`     | 모든 목록의 공통 외형. 수준 `n` 의 왼쪽 여백은 이 스타일의 왼쪽 여백 + `n` × 한 단계(이 스타일의 왼쪽 여백, 없으면 20pt), 첫 줄 내어쓰기는 이 스타일의 값. 이 스타일에 글머리표 목록(`kind: bullet`)이 있으면 글머리표 항목도 이 스타일로 씁니다 |
| `numbered_item` | 번호 항목. 번호 목록(`kind: number`)이 있어야 하고, 그 번호 정의는 첫 번호 정의(제목이 쓰는 개요)가 아니어야 하며, 수준 1–10 을 차례대로 모두 선언해야 합니다                                                                                    |
| `check_item`    | 체크 항목. 체크 글머리표 목록(`kind: check_bullet`, `checked_char` 와 `checkable: true` 를 가진 정의)이 있어야 합니다                                                                                                                            |
| `bullet_levels` | 수준 0–9 의 글머리표 정의 id 10 개(체크 글머리표는 안 됨). 없으면 `list_item` 의 글머리표를 모든 수준에서 씁니다                                                                                                                                 |

`numbered_item`·`check_item` 은 이름을 적었거나 같은 이름의 스타일이 있을 때만 정해집니다. 다른 스타일로 대신하지 않으므로, 둘 다 없는 템플릿은 번호·체크 항목을 목록이 아닌 문단으로 쓰고 `LIST_DEFINITION_MISSING` 을 냅니다. 정의 id 는 YAML 의 숫자 그대로가 아니라, 상속을 합친 뒤 그 id 의 정의가 놓인 자리로 찾습니다.

다음 경우는 템플릿 오류(`BlueprintError::StyleResolution`·`InvalidListReference`·`InvalidPlainBulletDefinition`)로 거절합니다.

- `numbered_item` 에 번호 목록이 없거나, 첫 번호 정의를 가리키거나, 그 번호 정의가 수준 1–10 을 차례대로 갖지 않을 때
- `check_item` 에 체크 글머리표 목록이 없을 때
- `bullet_levels` 가 10 개가 아니거나, 없는 id·체크 글머리표 id 를 담을 때
- `bullet_levels` 가 있는데 `list_item` 에 글머리표 목록이 없을 때

마지막 규칙은 상속에도 적용됩니다. 자식 템플릿이 `bullet_levels` 를 적지 않으면 부모의 값을 물려받고(빈 배열로 끌 수 없습니다), 자식의 값은 배열 전체를 바꿉니다. 그래서 `default` 를 상속하면서 `list_item` 을 글머리표 목록이 없는 스타일로 바꾸면, 물려받은 `bullet_levels` 때문에 템플릿이 거절됩니다. 그 스타일에 `list: { kind: bullet, … }` 를 더하세요.

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

## HwpxRegistryBridge 변환: from_registry()

Blueprint의 `StyleRegistry`를 HWPX 인코더 경계에서 안전하게 쓰기 위한 bridge를 만듭니다.
이 bridge는 두 가지를 함께 맡습니다.

- `HwpxStyleStore` 생성
- registry-local `CharShapeIndex` / `ParaShapeIndex`를 store-local HWPX id로 rebinding

```rust,no_run
use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_blueprint::registry::StyleRegistry;
use hwpforge_smithy_hwpx::HwpxRegistryBridge;

let template = builtin_default().unwrap();
let registry = StyleRegistry::from_template(&template).unwrap();

// Blueprint StyleRegistry → HWPX encode bridge
let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();
let style_store = bridge.style_store();
```

## 한컴 스타일셋: Classic / Modern / Latest

한글 프로그램은 버전에 따라 다른 기본 스타일 구성을 사용합니다.

| 스타일셋  | 스타일 수 | 설명                        |
| --------- | --------- | --------------------------- |
| `Classic` | 18개      | 한글 2014–2020              |
| `Modern`  | 22개      | **기본값** (한글 2022 이후) |
| `Latest`  | 23개      | 한글 2025 이후              |

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
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxRegistryBridge};
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
let bridge = HwpxRegistryBridge::from_registry(&registry).unwrap();

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

let rebound = bridge.rebind_draft_document(doc).unwrap();
let validated = rebound.validate().unwrap();
let image_store = Default::default();
let bytes = HwpxEncoder::encode(&validated, bridge.style_store(), &image_store).unwrap();
std::fs::write("report.hwpx", &bytes).unwrap();
```
