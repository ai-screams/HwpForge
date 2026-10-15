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
date: 2026-03-06          # Metadata.created (ISO 8601)
template: government      # 파싱되지만 스타일 선택에는 쓰이지 않습니다 (inert)
metadata:                 # 아래 하위 필드를 담는 중첩 맵
  subject: 신규 사업 제안   # Metadata.subject
  keywords:                # Metadata.keywords (YAML 배열)
    - 사업
    - 제안
  modified: 2026-03-10     # Metadata.modified (ISO 8601)
---
```

최상위 필드:

| 필드       | Metadata 필드 | 설명                                              |
| ---------- | ------------- | ------------------------------------------------- |
| `title`    | `title`       | 문서 제목                                         |
| `author`   | `author`      | 작성자                                            |
| `date`     | `created`     | 작성일 (ISO 8601)                                 |
| `template` | _(없음)_      | 파싱되지만 스타일 선택에는 쓰이지 않습니다(inert) |
| `metadata` | _(중첩 맵)_   | 아래 하위 필드를 담는 컨테이너                    |

`metadata:` 아래의 하위 필드:

| 하위 필드  | Metadata 필드 | 설명                    |
| ---------- | ------------- | ----------------------- |
| `subject`  | `subject`     | 주제/설명               |
| `keywords` | `keywords`    | 검색 키워드 (YAML 배열) |
| `modified` | `modified`    | 수정일 (ISO 8601)       |

`subject`/`keywords`/`modified`는 반드시 `metadata:` 아래에 중첩해야 합니다 — 최상위에 쓰면 조용히 무시됩니다(`Frontmatter` 구조체가 이 키들을 최상위 필드로 갖지 않고, 모르는 키를 에러로 거부하지도 않기 때문입니다).

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

## 각주 (Footnote) / 미주 (Endnote)

GFM 각주 문법으로 각주와 미주를 표현합니다.

```markdown
본문에 각주를 답니다.[^1] 미주도 답니다.[^e1]

[^1]: 각주 본문입니다.

[^e1]: 미주 본문입니다.
```

- `[^라벨]`: 각주 — 관례적으로 숫자를 씁니다(`[^1]`), 하지만 규칙 자체는 아래 미주 형태(`e[0-9]+`)가 아니면 어떤 라벨도 각주로 인정됩니다(예: `[^note]`도 유효한 각주 라벨입니다).
- `[^eN]` (`e` + 숫자 1개 이상): 미주 — HwpForge dialect의 예약 네임스페이스입니다. 각주 의도로 `[^e1]`을 써도 미주로 정규화됩니다 (lossy 변환입니다).
- 정의는 여러 문단으로 이어갈 수 있습니다. 첫 문단 다음에 빈 줄을 두고, 이어지는 문단의 모든 줄을 4-space 들여씁니다.

```markdown
[^1]: 첫 번째 문단.

    두 번째 문단 (4-space 들여쓰기).
```

`hwpforge to-md`(기본 `styled` 모드)로 HWPX → Markdown 역방향 변환할 때도 각주는 `[^N]`, 미주는 `[^eN]`으로 방출되어 왕복이 보존됩니다. `lossy` 모드는 대신 `(footnote: ...)`/`(endnote: ...)` 형태의 인라인 텍스트로 펼쳐서 출력하므로 이 왕복 규약의 대상이 아닙니다.

## 표 (Table)

GFM 표 문법을 지원합니다.

```markdown
| 항목 | 값     |
| ---- | ------ |
| 이름 | 홍길동 |
| 부서 | 기획팀 |
```

표 셀 안에는 링크, 이미지, 각주/미주 참조 등 인라인 요소도 담을 수 있습니다.

## 이미지

```markdown
![대체 텍스트](images/photo.png)
```

- 지원 포맷: PNG, JPEG, GIF, BMP, WMF, EMF (SVG는 지원하지 않습니다) — 확장자가 아니라 실제 바이트를 스니핑해 포맷을 확인합니다.
- 경로(상대·절대 모두)는 정규화한 뒤 **Markdown 파일이 있는 디렉터리 하위에 있는지** 검사합니다. `../`로 그 디렉터리를 벗어나는 경로는 거부되지만, 그 디렉터리 안을 가리키는 절대 경로는 허용됩니다 — 거부되는 것은 "절대 경로"가 아니라 "벗어나는 경로"입니다.
- `data:` URI(base64)도 지원합니다 — 네트워크 접근 없이 로컬에서 바로 디코드됩니다. 기준 디렉터리가 없는 인라인 텍스트/stdin 입력에서는 `data:` URI만 임베드할 수 있습니다.
- `http(s)` 원격 URL은 네트워크 접근을 금지하는 정책상 거부됩니다.
- 실패한 참조(파일 없음·경로 탈출·원격 URL·미지 포맷 등)는 경고와 함께 이미지 run이 드롭됩니다 — 결과 문서에 깨진 참조가 남지 않습니다.
- 파일 크기 상한은 50MB입니다.

## 구분선과 섹션 구분자

Markdown의 `---`(frontmatter 블록 밖에서 단독으로 쓴 thematic break)는 리터럴 `"---"` 문단으로 변환됩니다 — 페이지나 섹션을 나누는 구분자가 아닙니다. HWPX 섹션(다른 페이지 설정 구역)을 나누려면 반드시 `<!-- hwpforge:section -->` 주석을 사용하세요 (앞의 [섹션 마커](#섹션-마커) 참고).

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

| 모드              | 특징                                       | 용도                      |
| ----------------- | ------------------------------------------ | ------------------------- |
| `encode_lossy`    | 읽기 좋은 GFM                              | 사람이 읽는 문서 미리보기 |
| `encode_lossless` | 아래 보존 범위 안에서 보존, 못 담으면 거부 | 라운드트립, 백업          |

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
hwpforge = { version = "0.16", features = ["md"] }
```

### Lossy vs Lossless 모드 선택

| 기준          | Lossy (`encode_lossy`)      | Lossless (`encode_lossless`)                 |
| ------------- | --------------------------- | -------------------------------------------- |
| **출력 형식** | 표준 GFM Markdown           | YAML frontmatter + HTML 마크업               |
| **가독성**    | 높음 (사람/LLM 모두)        | 낮음 (기계 파싱용)                           |
| **정보 손실** | 스타일/레이아웃 일부 손실   | [보존 범위](#lossless-보존-범위) 안에서 보존 |
| **RAG 추천**  | **추천** — 청크 분할에 적합 | 원본 복원이 필요할 때만                      |
| **LLM 추천**  | **추천** — 토큰 효율적      | 라운드트립 편집 시                           |

Lossless 모드는 그림·도형·묶음의 개체 틀(원래 크기와 크기 고정)을 `data-original-width-unit`·`data-original-height-unit`·`data-size-protect` 속성으로, 표의 크기 고정을 `data-size-protect` 로 담아 되읽습니다(표에는 원래 크기가 없습니다). 이 속성들은 이전 버전의 디코더가 무시하므로, 이전 버전으로 되읽으면 개체 틀은 기본값이 됩니다.

### Lossless 보존 범위

Lossless 모드는 아래 "보존하는 것"을 그대로 되읽을 수 있게 쓰고, "거부하는 것"이 있으면 일부를 버리는 대신 문서 전체를 거부합니다(`ENCODE_FAILED`, Rust 는 `MdError::UnsupportedStructure`). **두 목록 밖의 값은 모두 거부 없이 빠집니다.**

| 구분                    | 내용                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 보존하는 것             | 구역의 쪽 크기·여백, 문단·run 구조와 글자, 문단 모양·글자 모양 번호(`para_shape_id`·`char_shape_id` — 문단 스타일 `style_id` 는 아님), 표의 행·칸(병합·너비·배경·줄 나눔 방식)·행 높이·표 너비·크기 고정·칸 크기 고정·셀 영역, 표 캡션의 글자·위치(측)·전체 폭 여부·저장된 너비, 링크(글자 링크와 그림 링크), 각주 본문, 그림·도형의 개체 틀(원래 크기·크기 고정), **그림**(본문·묶음 안 모두: 배치와 위치·바깥 여백, 크기, 자르기, 이미지 크기, 링크, 개체 번호, 그림 형식 이름 원문)과 본문 그림의 캡션(위치·간격·너비·전체 폭 여부·저장된 너비와 문단), **도형**(글상자·선·타원·다각형·사각형·호·곡선·연결선, 본문·묶음 안 모두: 정의점, 크기, 배치와 위치·바깥 여백, 선 색·두께·종류, 채우기(단색·그러데이션·무늬·없음), 회전·뒤집기·화살표·문단 첫 글자 장식·둥근 모서리, 글자의 세로 정렬, 글상자 안쪽 여백, 안의 문단), **묶음 개체**와 중첩 묶음 |
| 빠지는 것(예)           | 문단의 쪽·단 나눔·개요 수준·`style_id`·줄 배치 캐시, 탭 정보, 칸 여백·칸 세로 정렬, 표 캡션의 간격·너비와 글자가 아닌 내용, 구역의 단·쪽 번호 등 쪽 설정 밖의 값, 머리말·꼬리말·바탕쪽과 그 안의 개체, 각주·미주 번호                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| 거부하는 것             | 묶음 안의 수식·차트·글맵시, 도형의 캡션, 그림 채우기, 예약 비트가 켜진 도형 색·칸 배경색, 문단 없는 글상자·점 세 개 미만 다각형(어디에 있든), 32 겹을 넘는 중첩, 회전값 NaN·무한대, 묶음 안 그림의 캡션, 표 캡션 안의 그림·묶음·도형, 그림 캡션과 도형 글 안의 내용이 빈 문단·표·칸·각주, 그 밖에 이 형식이 표현하지 못하는 컨트롤                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| 쓰지만 되읽지 못하는 것 | `<endnote>`·`<dutmal>`·`<compose>` — 이 요소가 든 Markdown 은 `decode_lossless` 가 오류를 냅니다                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| 되읽기                  | Rust API `MdDecoder::decode_lossless` 만 읽습니다. CLI·MCP·Python 의 `convert` 는 lossless Markdown 을 읽지 않습니다                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

`<group>`·`<img>`·`<caption>` 과 도형 요소(`<textbox>`·`<line>`·`<ellipse>`·`<polygon>`·`<rect>`·`<arc>`·`<curve>`·`<connectline>`)는 정해진 속성만 받습니다 — 모르는 속성이나 같은 속성이 두 번 나오면 디코드가 실패합니다. 속성 값 안의 제어 문자(탭·줄바꿈 등)는 `&#9;` 같은 문자 참조로 써서 그대로 되읽습니다.

컨테이너(표·묶음·그림·캡션·도형·각주)는 32 겹까지 중첩할 수 있습니다. 더 깊은 문서는 인코드가, 더 깊은 Markdown 은 디코드가 오류로 거부합니다.

#### 버전·도구 사이의 호환

이 형식은 HwpForge 의 `MdDecoder::decode_lossless` 가 되읽으려고 쓰는 것입니다. 일반 Markdown 미리보기나 HTML 렌더러에서는 구조가 다르게 보일 수 있습니다.

| 이 버전이 쓴 것                                                                               | 이전 버전(v0.16.10) 디코더                                                           | 일반 HTML 렌더러                                                                                                                |
| --------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------- |
| 캡션 없는 `<img … />`(배치·개체 번호·자르기 등 속성이 늘어남)                                 | 읽지만 새 속성은 무시해 그 값을 잃음                                                 | 그림으로 보임                                                                                                                   |
| 캡션 있는 `<img …><caption>…</caption></img>`                                                 | 오류                                                                                 | `<img>` 는 자식을 가질 수 없어 캡션이 그림 밖으로 나옴. 표 칸 안이면 `<caption>` 이 칸을 닫아 바깥 표의 캡션으로 옮겨질 수 있음 |
| `<group>`·`<rect>`·`<arc>`·`<curve>`·`<connectline>`, 그리고 `<line>`·`<ellipse>`·`<polygon>` | 오류(`<line>`·`<ellipse>`·`<polygon>` 은 이전 버전이 쓰기는 했으나 읽지 못하던 태그) | 모르는 요소로 다룸                                                                                                              |
| `<textbox>` 의 새 속성(스타일·배치·세로 정렬·안쪽 여백)                                       | 읽지만 새 속성은 무시해 그 값을 잃음                                                 | —                                                                                                                               |
| `<td data-line-wrap>`                                                                         | 무시해 기본 줄 나눔이 됨                                                             | —                                                                                                                               |
| 속성 값의 `&#9;` 같은 문자 참조                                                               | 원래 글자로 읽음                                                                     | 원래 글자로 읽음                                                                                                                |

반대 방향으로, 이전 버전이 쓴 lossless Markdown 은 이 버전이 읽을 수 있는 범위에서 그대로 읽습니다 — 이전 출력의 속성은 모두 새 디코더가 받는 속성 목록에 들어 있습니다. 다만 `<endnote>`·`<dutmal>`·`<compose>` 는 여전히 읽지 못하고, 컨테이너가 32 겹을 넘게 중첩된 파일은 새 깊이 한도로 이제 오류가 납니다.

묶음 개체는 lossy·styled 모두 자식을 본문과 같은 방식으로 그 자리에 그립니다(그림은 링크, 글상자는 그 글). lossy 는 글상자·도형·각주 글자 안의 그림과 묶음을 그리지 않습니다(그 글은 평문). styled 는 쓴 그림 링크마다 그 그림 파일을 `images` 에 함께 내며, 메모(`<!-- memo: … -->`) 안의 그림은 링크 글자만 씁니다. 병합 칸(HTML 표) 안의 글상자 글은 HTML 서식(`<strong>` 등)으로 씁니다. 각주 안이나 메모 안의 각주는 `(주: …)` 로 본문에 넣습니다. 중첩 깊이는 보기마다 세는 법이 다릅니다. lossless 와 lossy 는 문서 구조의 깊이를 lossless 디코더와 같은 방식으로 셉니다 — 표·그림(캡션은 한 겹 더)·묶음·도형·각주·메모가 한 겹씩이고, 32 겹을 넘으면 오류를 냅니다. styled 는 그리면서 들어가는 깊이를 셉니다 — 표·캡션·도형 글·각주·메모만 한 겹씩이고 묶음과 그림 자체는 세지 않으므로, 같은 문서라도 lossless·lossy 가 거부하는 깊이를 styled 는 그릴 수 있습니다(예: 묶음 → 글상자 17번 반복은 구조 깊이 34 라 lossy 는 거부, styled 는 글상자 17 겹이라 그림). styled 는 32 겹을 넘는 자리에 오류 대신 `(중첩 깊이 초과로 생략)` 을 씁니다. lossy 는 표 안의 표가 8 단계를 넘으면 그 자리에 `(중첩 표 생략)` 을 씁니다. 생략한 자리는 경고로 알립니다 — Rust 는 `MdOutput::warnings`·`encode_lossy_with_report` 의 `MdWarning::ContainerTooDeep`, CLI·MCP·Python 은 경고 코드 `CONTAINER_TOO_DEEP`. styled 는 수식을 `$…$` LaTeX 로 쓰되 Markdown 글에서는 `<`·`>` 를 `\lt`·`\gt` 로 바꾸고, LaTeX 에 `$`·`](` 가 있거나 `\` 로 끝나면 수식 스크립트를 일반 글자로 쓴 뒤 `MdWarning::EquationAsText`(경고 코드 `EQUATION_AS_TEXT`)로 알립니다.

#### 읽기 보기(styled·lossy)가 escape 하는 것과 쓰는 HTML

문서 글자는 그 글자가 놓이는 자리(Markdown 글, GFM 표 칸, HTML 칸, 코드)에 맞게 한 번만 escape 해서 씁니다. 두 보기 모두 다음은 같습니다.

- **줄 끝:** CR, LF, CRLF 를 모두 같은 줄 끝으로 다룹니다. Markdown 글에서는 줄바꿈, GFM 칸과 HTML 칸에서는 `<br>` 로 씁니다.
- **링크:** 주소가 `http://`·`https://`·`mailto:`(대소문자 무관)이거나 비어 있고, 공백·제어 문자가 없을 때만 링크로 씁니다. 상대 경로, `#앵커`, 그 밖의 스킴은 표시 글자만 씁니다. 이전 버전은 이런 링크도 썼으므로 동작이 바뀐 부분입니다.
- **링크 주소·그림 경로:** 다시 읽었을 때 원래 값과 같도록 씁니다. `\`·`(`·`)`·`<`·`>`, 그리고 엔티티 앞의 `&` 는 앞에 `\` 를 붙입니다. 공백이 있으면 `<…>` 형식으로 씁니다. 줄 끝은 주소 안에 둘 수 없어서 `%0A`·`%0D` 로 씁니다. 링크 글자와 그림 대체 글자 안의 줄 끝은 공백으로 씁니다.
- **코드:** 코드 블록과 코드 스팬의 울타리는 내용에 있는 가장 긴 백틱 연속보다 깁니다. 그래서 내용이 울타리를 닫지 못합니다.

styled 는 글자에 Markdown escape 를 적용합니다. `*`·`_`·`` ` ``·`[`·`]`·`~`·`\`·`#` 앞에 `\` 를 붙이고, `<`·`>` 는 `&lt;`·`&gt;`, 엔티티 앞의 `&` 는 `\&` 로 씁니다. 서식 태그(`<u>` 등) 안의 글자도 같은 방식으로 escape 합니다. Markdown 렌더러는 인라인 HTML 태그 안의 Markdown 도 해석하기 때문입니다. HTML 칸의 글자는 HTML escape 합니다.

수식은 `$…$` LaTeX 로 씁니다. 이때 보장하는 범위는 실행 가능한 HTML 과 검사하지 않은 링크를 만들지 않는 것까지입니다. `<`·`>` 는 `\lt`·`\gt` 로 바꿉니다. LaTeX 에 `$`·`](` 가 있거나 `\` 로 끝나면 수식 스크립트를 글자로 쓰고 `EQUATION_AS_TEXT` 로 알립니다. `$` 안의 다른 Markdown(`*x*` 등)은 수식 렌더러가 먼저 읽는다고 보고 글자로 바꾸지 않습니다. 중첩이 너무 깊어 변환하지 못한 수식도 글자로 쓰지만, 이 경우에는 경고를 내지 않습니다.

lossy 는 문서 글자를 거의 그대로 씁니다. CommonMark 가 글자가 아닌 마크업으로 읽는 자리에만 앞에 `\` 를 붙입니다. 판정은 문단 단위로 합니다. 그래서 여러 run 에 걸친 시퀀스(`[a]` 다음 run 의 `(…)`)와, 원문 글자와 생성한 마크업이 맞닿아 생기는 문법(원문 `!` 바로 뒤의 생성 링크, 원문 끝의 `\` 바로 뒤의 생성 링크)도 잡습니다.

| escape 하는 것                                                                                                                                                               | 예                                                  |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| raw HTML 을 여는 `<` — 완전한 여는 태그(이름, 속성, `>`·`/>`), 닫는 태그, 주석, `<?`, 선언, CDATA, HTML 블록을 여는 태그 이름(`div`·`table`·`script` 등 뒤에 공백·`>`·줄 끝) | `<b>`, `</b>`, `<The Pulse>`, `<!-- -->`, `<div 가` |
| 자동 링크를 여는 `<`                                                                                                                                                         | `<https://…>`, `<a@b>`                              |
| 엔티티·문자 참조를 여는 `&`                                                                                                                                                  | `&amp;`, `&#60;`                                    |
| 인라인 링크·그림 이름을 닫는 `]`(뒤에 `(`)                                                                                                                                   | `[현재단계](기관)`                                  |
| 줄 머리의 링크 참조 정의 `[`                                                                                                                                                 | `[a]: …`                                            |
| 링크 글자·그림 대체 글자의 `\`·`[`·`]`·`` ` ``                                                                                                                               | —                                                   |

위에 해당하지 않는 글자는 그대로 둡니다. 예를 들어 `<붙임>`, `<표 1>`, `<AI가 바꾼 것>`, `a < b`, `[참고]`, `AT&T`, `*강조*`, `# 제목`, `1. 항목` 은 그대로 씁니다. 그래서 `*`·`_`·`#`·`-`·숫자 목록은 Markdown 서식으로 보일 수 있습니다. lossy 는 읽기 위한 보기라서 이 차이는 받아들였습니다. escape 하는 글자 바로 앞에 원래 있던 역슬래시는 두 배로 써서 글자로 남깁니다.

두 보기가 직접 쓰는 HTML 은 아래 것뿐입니다. 이 밖의 HTML 은 문서 글자에서 생기지 않습니다(`tests/markup_safety.rs` 가 공격 문자열을 모든 자리에 넣고 출력을 파싱해 확인합니다).

| HTML                                                            | 쓰는 곳                                                         |
| --------------------------------------------------------------- | --------------------------------------------------------------- |
| `<br>`                                                          | 표 칸 안 줄바꿈(GFM 칸·HTML 칸), HTML 칸 캡션                   |
| `<table>`·`<tbody>`·`<tr>`·`<td colspan rowspan>`               | styled 병합 칸·표 안 표                                         |
| `<strong>`·`<em>`·`<del>`·`<u>`·`<sup>`·`<sub>`                 | styled 서식                                                     |
| `<a href>`·`<img src alt>`                                      | styled HTML 칸 링크·그림(속성은 HTML escape, 줄 끝은 문자 참조) |
| `<!-- hwpforge:section -->`·`<!-- chart -->`·`<!-- memo: … -->` | 구역 경계·차트 자리·메모(메모 본문의 `--` 는 `\-\-`)            |

styled 의 결과 `MdOutput` 에는 `markdown`·`images`·`warnings` 세 필드가 있습니다. `#[non_exhaustive]` 구조체라서 크레이트 밖에서는 구조체 리터럴로 만들 수 없고, `MdEncoder::encode_styled` 로만 만듭니다. 필드를 읽거나 `let MdOutput { markdown, images, .. } = out;` 처럼 `..` 를 넣어 분해합니다.

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

# HWPX → Markdown
hwpforge to-md report.hwpx -o report.md

# 변환 모드 선택 (기본값: styled)
hwpforge to-md report.hwpx -o report.md --mode lossy
hwpforge to-md report.hwpx -o report.md --mode lossless

# HWPX 구조 확인 후 JSON으로 추출 (Markdown 변환 대안)
hwpforge inspect document.hwpx --json
hwpforge to-json document.hwpx -o document.json
```

> **참고**: CLI의 `convert` 명령은 Markdown → HWPX 방향만 지원합니다. HWPX → Markdown 변환은 `hwpforge to-md` 명령(모드 `styled`/`lossy`/`lossless`) 또는 Rust API(`MdEncoder`)를 사용하세요.
