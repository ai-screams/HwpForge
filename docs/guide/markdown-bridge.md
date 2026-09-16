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
