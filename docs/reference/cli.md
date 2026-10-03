# CLI 레퍼런스

`hwpforge`(Hammer)는 HwpForge의 명령줄 도구입니다. 이 장은 `hwpforge --help`와 `hwpforge <명령> --help`가 보여 주는 23개 명령을 정리합니다. 플래그 이름과 기본값은 clap 정의(`crates/hwpforge-bindings-cli/src/main.rs`)를 그대로 옮겼으므로, 버전이 올라 의심스러우면 `--help`가 우선입니다.

## 설치

`hwpforge-bindings-cli`는 crates.io에 배포하지 않으므로(`publish = false`) git 또는 로컬 경로에서 설치합니다. `hwpforge-smithy-pdf`(krilla) 의존 때문에 워크스페이스 MSRV(1.89)보다 높은 **Rust 1.92 이상이** 필요합니다(`rust-version = "1.92"`).

```console
cargo install --git https://github.com/ai-screams/HwpForge hwpforge-bindings-cli

# 또는 clone 후 로컬 경로에서
git clone https://github.com/ai-screams/HwpForge && cd HwpForge
cargo install --path crates/hwpforge-bindings-cli
```

설치되는 바이너리 이름은 `hwpforge`입니다(`[[bin]] name = "hwpforge"`).

## 전역 옵션

| 옵션              | 설명                                        |
| ----------------- | ------------------------------------------- |
| `--json`          | 결과를 machine-readable JSON으로 출력합니다 |
| `-h`, `--help`    | 도움말                                      |
| `-V`, `--version` | 버전                                        |

`--json`은 모든 명령이 받습니다. 에이전트가 결과를 파싱할 때는 이 플래그를 붙이세요.

## 명령 한눈에 보기

`--help`는 HWP5·PDF 명령을 먼저 나열하고 그 뒤에 HWPX 명령을 나열합니다. 아래 표는 용도별로 다시 묶었습니다.

| 용도          | 명령                                                                             |
| ------------- | -------------------------------------------------------------------------------- |
| HWP5 · PDF    | `audit-hwp5`, `census-hwp5`, `convert-hwp5`, `to-pdf`                            |
| 만들기 · 변환 | `convert`, `to-json`, `from-json`, `to-md`                                       |
| 읽기 · 검사   | `inspect`, `outline`, `fields`, `read`, `diff`, `validate`                       |
| 편집          | `fill`, `set-cell`, `patch`, `insert-para`, `delete-para`, `stamp-plan`, `stamp` |
| 부가          | `templates`, `schema`                                                            |

## HWP5 · PDF

### convert-hwp5

HWP5(`.hwp`)를 HWPX로 변환합니다.

```console
hwpforge convert-hwp5 [OPTIONS] --output <OUTPUT> <INPUT>
```

| 옵션                   | 설명                                                                                                 |
| ---------------------- | ---------------------------------------------------------------------------------------------------- |
| `-o`, `--output`       | 출력 HWPX 경로(필수)                                                                                 |
| `--carry-layout-cache` | HWP5의 조판 캐시(`PARA_LINE_SEG`)를 HWPX `<hp:linesegarray>`로 실어 `to-pdf`로 렌더할 수 있게 합니다 |

`--carry-layout-cache`로 만든 결과는 **PDF 재생·대조 전용입니다**. 한컴에서 다시 여는 용도로 쓰면 여러 줄이 겹칠 위험이 있다고 `--help`가 밝힙니다. 좌표를 정규화할 수 없는 문단(차트, 알 수 없는 컨트롤, 모호한 마커 경계)은 잘못된 데이터를 싣는 대신 `LAYOUT_CACHE_DROPPED` 경고와 함께 캐시를 버립니다.

### to-pdf

HWPX 또는 HWP5 문서를 조판 캐시 재생 방식으로 PDF로 렌더합니다. 입력 형식은 확장자가 아니라 내용으로 판별합니다.

```console
hwpforge to-pdf [OPTIONS] <INPUT>
```

| 옵션                      | 설명                                                                                                              |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `-o`, `--output`          | 출력 PDF 경로(기본값: 입력 경로의 확장자를 `.pdf`로 바꾼 것)                                                      |
| `--font-dir <FONT_DIRS>`  | 폰트를 찾을 디렉터리(반복 가능)                                                                                   |
| `--discovery <DISCOVERY>` | 폰트 자동 탐색: `explicit`(결정적, 기본값) \| `hancom` \| `platform`                                              |
| `--degraded`              | 렌더 실패를 오류 대신 경고로 낮춥니다. 없는 스타일·축 폰트는 regular로 그리고, 렌더할 수 없는 이미지는 건너뜁니다 |
| `--partial-cache-reject`  | 조판 캐시가 없는 문단이 하나라도 있으면 문서를 거부합니다(기본값: 경고 후 그 문단을 건너뜀)                       |

조판 캐시가 있는 문서만 렌더할 수 있습니다. `convert`나 `from-json`이 새로 만든 문서에는 캐시가 없어 거부됩니다.

### audit-hwp5

HWP5 원본과 변환된 HWPX 결과의 구조·의미 동등성을 점검합니다. 픽셀 단위 시각·쪽 나눔 동등성은 보장하지 않으며, 후속 시각 검증용 체크리스트를 보고서에 담습니다.

```console
hwpforge audit-hwp5 [OPTIONS] <SOURCE> <RESULT>
```

`<SOURCE>`는 원본 `.hwp`, `<RESULT>`는 변환된 `.hwpx`입니다.

### census-hwp5

HWP5 파일(과 선택적 HWPX 짝 파일)의 원시 fixture census를 만듭니다.

```console
hwpforge census-hwp5 [OPTIONS] <INPUT>
```

| 옵션                      | 설명                                         |
| ------------------------- | -------------------------------------------- |
| `--companion <COMPANION>` | 중첩 XML·경로 census용 HWPX 짝 fixture(선택) |
| `-o`, `--output`          | census 데이터셋을 JSON으로 저장할 경로(선택) |

## 만들기 · 변환

### convert

Markdown을 HWPX로 변환합니다.

```console
hwpforge convert [OPTIONS] --output <OUTPUT> <INPUT>
```

| 옵션                | 설명                                         |
| ------------------- | -------------------------------------------- |
| `<INPUT>`           | Markdown 파일. `-`를 주면 stdin에서 읽습니다 |
| `-o`, `--output`    | 출력 HWPX 경로(필수)                         |
| `--preset <PRESET>` | 스타일 프리셋 이름(기본값: `default`)        |

사용 가능한 프리셋은 `templates list`로 확인합니다.

### to-json

HWPX를 편집 가능한 JSON으로 내보냅니다.

```console
hwpforge to-json [OPTIONS] --output <OUTPUT> <FILE>
```

| 옵션                  | 설명                                                 |
| --------------------- | ---------------------------------------------------- |
| `-o`, `--output`      | 출력 JSON 경로(**필수**, stdout 내보내기는 없습니다) |
| `--section <SECTION>` | 특정 섹션만 추출(0부터 시작하는 인덱스)              |
| `--no-styles`         | 스타일 정보를 제외합니다                             |

`--section` 없이 내보낸 **문서 전체 JSON은** `from-json`이, `--section N`으로 내보낸 **섹션 JSON은** `patch`가 읽습니다. 두 JSON은 서로 다른 형식입니다.

### from-json

JSON을 HWPX로 되돌립니다.

```console
hwpforge from-json [OPTIONS] --output <OUTPUT> <INPUT>
```

| 옵션             | 설명                                       |
| ---------------- | ------------------------------------------ |
| `-o`, `--output` | 출력 HWPX 경로(필수)                       |
| `--base <BASE>`  | 이미지를 물려받을 기준 HWPX(왕복 충실도용) |

### to-md

HWPX를 Markdown으로 변환합니다.

```console
hwpforge to-md [OPTIONS] <INPUT>
```

| 옵션             | 설명                                                                                                                                   |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `-o`, `--output` | 출력 **디렉터리**(기본값: 입력과 같은 디렉터리)                                                                                        |
| `--mode <MODE>`  | `styled`(기본값, 스타일을 따르고 이미지 포함) \| `lossy`(스타일 정보 없는 읽기용) \| `lossless`(YAML 프론트매터가 붙은 왕복 안전 형식) |

## 읽기 · 검사

### inspect

디코드한 HWPX 문서 구조를 보여 줍니다. **HWPX만** 받습니다. HWP5는 먼저 `convert-hwp5`로 변환하거나 `audit-hwp5`로 비교하세요.

```console
hwpforge inspect [OPTIONS] <FILE>
```

| 옵션       | 설명                                     |
| ---------- | ---------------------------------------- |
| `--styles` | 스타일 상세(문자·문단 모양)를 포함합니다 |

### outline

문서의 내비게이션 맵(제목, 표, 이름 있는 누름틀, 책갈피)을 보여 줍니다. 이름 앵커(제목 텍스트, 표 순번, 필드·책갈피 이름)가 1차 키이고 `{section, para}` 위치는 구조 편집 뒤에 낡을 수 있으므로, 읽기·편집 전에 한 번 받아 두는 용도입니다.

```console
hwpforge outline [OPTIONS] <FILE>
```

### fields

이름 있는 누름틀(click-here field)과 각각이 채울 수 있는 필드인지 나열합니다.

```console
hwpforge fields [OPTIONS] <FILE>
```

### read

문서 전체를 내보내지 않고 목표 하나만 텍스트로 읽습니다. 목표는 정확히 하나만 지정합니다.

```console
hwpforge read [OPTIONS] <FILE>
```

| 옵션             | 설명                                                                             |
| ---------------- | -------------------------------------------------------------------------------- |
| `--section <N>`  | 문단을 읽을 섹션 인덱스                                                          |
| `--paras <A..B>` | 닫힌 문단 범위 `"A..B"` 또는 단일 `"N"`(`--section` 필요)                        |
| `--table <N>`    | 표 순번. 논리 격자 텍스트 행렬로 읽습니다(병합 영역은 앵커에 한 번, span과 함께) |
| `--field <NAME>` | 이름 있는 누름틀 하나                                                            |

텍스트가 아닌 내용(표, 이미지, 컨트롤)은 조용히 버려지지 않고 명시적인 표지로 드러납니다. 읽기 전용입니다.

### diff

두 HWPX를 두 채널로 비교해 편집이 실제로 무엇을 바꿨는지 확인합니다.

```console
hwpforge diff [OPTIONS] <BASE> <REVISED>
```

| 옵션             | 설명                                  |
| ---------------- | ------------------------------------- |
| `-o`, `--output` | 전체 JSON 보고서를 이 경로에도 씁니다 |

`semantic` 채널은 디코드한 Core 구조를 필드 값, 표 셀 텍스트 `{table, row, col}`, 문단 텍스트 `{section, para}`, 구조 개수, 상한이 있는 미분류 잔여로 분류합니다. `package` 채널은 ZIP 엔트리를 바이트로 비교합니다. 바뀐 엔트리 안의 wire 내용(예: `hp:linesegarray` 조판 캐시)은 항목별로 나열하지 않으며 보고서가 그렇게 밝힙니다. `fill`·`set-cell`·`stamp`·`patch` 뒤에 돌려 의도한 변경만 반영됐는지 확인하세요.

### validate

HWPX가 디코드되고 Core의 구조 불변식(`Document::validate`)을 만족하는지 편집 없이 검사합니다.

```console
hwpforge validate [OPTIONS] <FILE>
```

종료 코드는 `--help`에 다음과 같이 정의돼 있습니다.

| 종료 코드 | 의미                                                                                         |
| --------- | -------------------------------------------------------------------------------------------- |
| 0         | 문서가 유효합니다                                                                            |
| 1         | 파일 또는 인자 오류(없거나 읽을 수 없는 파일)                                                |
| 2         | 바이트가 디코드 가능한 HWPX 패키지가 아닙니다                                                |
| 3         | 디코드는 되지만 검증에 실패합니다(오류가 아니라 판정이며, `--json` 출력의 `errors`를 보세요) |

## 편집

모든 편집 명령은 결과를 `-o`로 지정한 출력 경로에 씁니다(필수 옵션). 편집 뒤에는 `diff`로 변경 범위를 확인하세요.

### fill

이름 있는 누름틀을 값으로 채웁니다. 건드리지 않는 패키지 엔트리는 바이트 그대로 보존합니다.

```console
hwpforge fill [OPTIONS] --output <OUTPUT> <FILE>
```

| 옵션                 | 설명                       |
| -------------------- | -------------------------- |
| `--set <NAME=VALUE>` | 채울 이름=값 쌍(반복 가능) |
| `-o`, `--output`     | 출력 HWPX 경로(필수)       |

요청한 값을 모두 먼저 검증(알 수 없는·중복된·채울 수 없는 이름, 빈 값)하고, 하나라도 실패하면 아무것도 쓰지 않습니다. **누름틀이 들어 있는 서식 문서에서만** 동작하므로 `convert`로 막 만든 문서에는 필드가 없습니다. 이름은 `fields`로 먼저 확인하세요.

### set-cell

표 셀을 논리 격자 주소로 편집합니다. 실패 시 아무것도 쓰지 않는 admission 게이트 뒤에서 동작합니다.

```console
hwpforge set-cell [OPTIONS] --output <OUTPUT> <FILE>
```

| 옵션                 | 설명                                                                         |
| -------------------- | ---------------------------------------------------------------------------- |
| `--table <N>`        | 표 순번(문서 순서, 0부터)                                                    |
| `--at <R,C>`         | 격자 좌표 `"row,col"`(덮인 위치는 병합 앵커로 해석)                          |
| `--right-of <LABEL>` | 유일하게 라벨된 셀의 오른쪽 셀을 대상으로 합니다                             |
| `--below <LABEL>`    | 유일하게 라벨된 셀의 아래쪽 셀을 대상으로 합니다                             |
| `--text <TEXT>`      | 바꿀 텍스트(빈 문자열은 셀을 비웁니다)                                       |
| `--map <MAP_JSON>`   | 일괄 편집용 명세 맵(`CellSpec` JSON 배열). 위 플래그들과 함께 쓸 수 없습니다 |
| `-o`, `--output`     | 출력 HWPX 경로(필수)                                                         |

### patch

기존 HWPX의 섹션 하나를 교체합니다.

```console
hwpforge patch [OPTIONS] --section <SECTION> --output <OUTPUT> <BASE> <SECTION_JSON>
```

`<SECTION_JSON>`은 `to-json --section N`으로 내보낸 **섹션 JSON이며**, `<BASE>`가 이미지와 나머지 패키지의 기준입니다. 문서 전체 JSON을 넣으면 스키마 불일치로 거부됩니다. 구조(문단 수)를 바꾸려면 `insert-para`·`delete-para`나 `from-json`을 쓰세요.

### insert-para

기준 문단 앞이나 뒤에 새 최상위 문단을 삽입합니다. 다른 바이트는 그대로 둡니다.

```console
hwpforge insert-para [OPTIONS] --section <SECTION> --anchor <ANCHOR> --text <TEXTS> --output <OUTPUT> <FILE>
```

| 옵션             | 설명                                                                                   |
| ---------------- | -------------------------------------------------------------------------------------- |
| `--section <N>`  | 섹션 인덱스                                                                            |
| `--anchor <N>`   | 기준 문단 인덱스(새 문단이 이 문단의 문단·문자 모양을 그대로 물려받습니다)             |
| `--before`       | 기준 앞에 삽입합니다(기본값은 뒤)                                                      |
| `--text <TEXTS>` | 새 문단의 한 줄 일반 텍스트. 반복하면 연속된 블록을 한 번의 검증된 편집으로 삽입합니다 |
| `-o`, `--output` | 출력 HWPX 경로(필수)                                                                   |

섹션의 첫 문단(secPr를 담은 문단) 앞 삽입은 거부됩니다. 왕복 안전한 입력만 편집할 수 있습니다.

### delete-para

최상위 본문 문단을 인덱스로 삭제합니다. 전부 성공하거나 전부 취소됩니다.

```console
hwpforge delete-para [OPTIONS] --section <SECTION> --output <OUTPUT> <FILE>
```

| 옵션             | 설명                                        |
| ---------------- | ------------------------------------------- |
| `--section <N>`  | 섹션 인덱스                                 |
| `--index <N>...` | 삭제할 최상위 문단 인덱스(일괄 삭제는 반복) |
| `-o`, `--output` | 출력 HWPX 경로(필수)                        |

참조(책갈피·상호참조·각주 등)를 담은 문단, 강제 쪽·단 나눔이 있는 문단, 섹션 속성(secPr)을 담은 첫 문단, 삭제하면 섹션이 비게 되는 경우는 거부합니다(fail-closed).

### stamp-plan

산문 속 자리표시자(체크박스, 괄호 빈칸, 날짜 빈칸, 단독 `@`, 도장 토큰) 후보를 찾아 템플릿 스탬핑 계획을 만듭니다.

```console
hwpforge stamp-plan [OPTIONS] <FILE>
```

출력의 각 후보에 `{"field":{"name":"…"}}` 또는 `"ignore"` 액션을 붙여 명세 맵을 만든 뒤 `stamp --map`을 실행합니다. 지침 문맥의 후보(guarded)는 자동 적용되지 않습니다.

### stamp

명세 맵을 적용해 자리표시자를 이름 있는 누름틀로 승격합니다. 전부 성공하거나 전부 취소되며, 스탬프된 HWPX와 매니페스트를 씁니다.

```console
hwpforge stamp [OPTIONS] --map <MAP_JSON> --output <OUTPUT> <FILE>
```

| 옵션                    | 설명                                                   |
| ----------------------- | ------------------------------------------------------ |
| `--map <MAP_JSON>`      | `StampSpec` JSON 배열(필수)                            |
| `-o`, `--output`        | 출력 HWPX 경로(필수)                                   |
| `--manifest <MANIFEST>` | 매니페스트 JSON 경로(기본값: `<output>.manifest.json`) |

실패 시 아무것도 쓰지 않는 admission 게이트(무변경 왕복 + ZIP 닫힌 세계 검사) 뒤에서 동작하고, 결과물은 곧바로 `fields`·`fill`에 쓸 수 있습니다.

## 부가

### templates

스타일 프리셋을 관리합니다. 하위 명령은 두 개입니다.

```console
hwpforge templates list            # 사용 가능한 프리셋 목록
hwpforge templates show <NAME>     # 프리셋 상세
```

### schema

문서·스타일 타입의 JSON Schema를 출력합니다.

```console
hwpforge schema [OPTIONS] [TYPE_NAME]
```

`TYPE_NAME`은 `document`(기본값), `exported-document`, `exported-section` 중 하나입니다. `exported-document`가 `to-json`의 전체 JSON, `exported-section`이 `--section` JSON의 형식입니다.

## 다음 단계

- 같은 연산을 AI 도구에서 부르려면 [MCP 서버 레퍼런스](./mcp.md)를 보세요.
- Python에서 부르려면 [Python 가이드](../guide/python.md)를 보세요.
