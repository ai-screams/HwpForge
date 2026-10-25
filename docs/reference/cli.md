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

| 옵션                | 설명                                                                                   |
| ------------------- | -------------------------------------------------------------------------------------- |
| `-o`, `--output`    | 출력 HWPX 경로(필수)                                                                   |
| `--no-layout-cache` | HWP5의 조판 캐시(`PARA_LINE_SEG`)를 싣지 않습니다. 줄 나눔은 여는 쪽이 다시 계산합니다 |

기본값은 조판 캐시를 HWPX `<hp:linesegarray>`로 싣습니다. 그래서 결과가 원본의 줄 나눔 자리를 담고 `to-pdf`의 렌더 대상이 됩니다. 줄 시작 자리를 정확히 옮길 수 없는 문단(차트, 알 수 없는 컨트롤, 경계로 가를 수 없는 줄 시작)은 잘못된 데이터를 싣는 대신 `LAYOUT_CACHE_DROPPED` 경고와 함께 캐시를 버립니다. 예전 옵션 `--carry-layout-cache`는 계속 받지만 기본값과 같은 출력을 내며, `--no-layout-cache`와 함께 줄 수 없습니다.

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

0.17.0 부터 두 JSON 모두 맨 위에 `"format_version": 2` 를 쓰고, 문단 여백 다섯 가지(`margin_left`, `margin_right`, `indent`, `spacing_before`, `spacing_after`)와 탭 위치(`position`)를 단위와 값으로 씁니다. `hwpunit` 은 1/100 pt, `char` 는 1/500 글자입니다.

```json
"margin_left": { "unit": "hwpunit", "value": 2000 },
"indent": { "unit": "char", "value": -1000 }
```

`from-json` 과 `patch` 는 이전 판이 쓴 버전 없는 JSON 을 이렇게 다룹니다.

- 탭 위치가 하나라도 있으면 `LEGACY_JSON_TABS` 로 거부합니다. 옛 JSON 에는 탭 단위가 남아 있지 않아 추측하지 않습니다.
- 탭 위치가 없으면 문단 여백을 옛 표현에서 풀어 읽고 `LEGACY_PARAGRAPH_MEASURES` 경고를 냅니다. 옛 JSON 이 이미 잃은 단위는 되살리지 못합니다.
- 다른 버전 번호나 정수가 아닌 `format_version` 은 거부합니다.

어느 경우든 원본 HWPX 또는 HWP5 에서 새 판의 `to-json` 을 다시 실행하는 것이 가장 정확합니다.

스타일 글자 모양의 그림자는 0.17.0 부터 `shadow` 객체로 나옵니다(이전 키 `shadow_type` 은 종류만 담았습니다).

```json
"shadow": { "kind": "Continuous", "color": "#123456", "offset_x": -3, "offset_y": 4 }
```

`color` 는 `"#RRGGBB"` 또는 `"none"` 입니다. 이전 판이 쓴 `"shadow_type": "Drop"` JSON 도 `from-json` 이 읽고, 이때 색·간격은 기본값 `#C0C0C0`·10·10 이 됩니다(이전 출력 `#B2B2B2` 보다 밝음).

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

| 옵션             | 설명                                                                                                                                                                                                                                                                             |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-o`, `--output` | 출력 **디렉터리**(기본값: 입력과 같은 디렉터리)                                                                                                                                                                                                                                  |
| `--mode <MODE>`  | `styled`(기본값, 스타일을 따르고 이미지 포함) \| `lossy`(스타일 정보 없는 읽기용) \| `lossless`(YAML 프론트매터가 붙은 왕복 형식 — [보존 범위](../guide/markdown-bridge.md#lossless-보존-범위)의 보존 목록은 보존, 거부 목록에 든 개체가 있으면 오류, 그 밖의 값은 빠질 수 있음) |

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

| 옵션                  | 설명                                                                             |
| --------------------- | -------------------------------------------------------------------------------- |
| `--section <SECTION>` | 문단을 읽을 섹션 인덱스                                                          |
| `--paras <PARAS>`     | 닫힌 문단 범위 `"A..B"` 또는 단일 `"N"`(`--section` 필요)                        |
| `--table <TABLE>`     | 표 순번. 논리 격자 텍스트 행렬로 읽습니다(병합 영역은 앵커에 한 번, span과 함께) |
| `--field <FIELD>`     | 이름 있는 누름틀 하나                                                            |

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

| 종료 코드 | 의미                                                                                                                                                                                                           |
| --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0         | 문서가 유효합니다                                                                                                                                                                                              |
| 1         | 파일을 읽을 수 없음(없거나 읽을 수 없거나 크기 상한을 넘는 파일)                                                                                                                                               |
| 2         | 명령행 인자가 잘못됐거나 바이트가 디코드 가능한 HWPX 패키지가 아닙니다                                                                                                                                         |
| 3         | 디코드는 되지만 검증에 실패합니다(오류가 아니라 판정이며, `--json` 출력의 `errors`를 보세요). 파서는 통과하고 Core 중첩 깊이가 32를 넘는 문서는 디코드에서 거부되므로 3이 아니라 `DECODE_FAILED`(종료 2)입니다 |

인자를 잘못 준 경우(필수 인자 누락, 알 수 없는 옵션)는 clap이 종료 코드 2로 끝냅니다. 아래 오류 계약은 다른 명령의 종료 정책도 함께 설명합니다.

## 편집

`fill`·`set-cell`·`insert-para`·`delete-para`·`stamp`·`patch`는 결과를 `-o`로 지정한 출력 경로에 씁니다(필수 옵션). `stamp-plan`은 파일을 쓰지 않으므로 `-o`가 없습니다. 편집 뒤에는 `diff`로 변경 범위를 확인하세요.

### fill

이름 있는 누름틀을 값으로 채웁니다. 건드리지 않는 패키지 엔트리는 바이트 그대로 보존합니다.

```console
hwpforge fill [OPTIONS] --output <OUTPUT> <FILE>
```

| 옵션                 | 설명                                       |
| -------------------- | ------------------------------------------ |
| `--set <NAME=VALUE>` | 채울 이름=값 쌍(반복 가능). 하나 이상 필수 |
| `-o`, `--output`     | 출력 HWPX 경로(필수)                       |

요청한 값을 모두 먼저 검증(알 수 없는·중복된·채울 수 없는 이름, 빈 값)하고, 하나라도 실패하면 아무것도 쓰지 않습니다. **누름틀이 들어 있는 서식 문서에서만** 동작하므로 `convert`로 막 만든 문서에는 필드가 없습니다. 이름은 `fields`로 먼저 확인하세요.

### set-cell

표 셀을 논리 격자 주소로 편집합니다. 실패 시 아무것도 쓰지 않는 admission 게이트 뒤에서 동작합니다.

```console
hwpforge set-cell [OPTIONS] --output <OUTPUT> <FILE>
```

| 옵션                 | 설명                                                                             |
| -------------------- | -------------------------------------------------------------------------------- |
| `--table <TABLE>`    | 표 순번(문서 순서, 0부터). 단일 편집에는 필수(`--map` 일괄 편집에서는 쓰지 않음) |
| `--at <R,C>`         | 격자 좌표 `"row,col"`(덮인 위치는 병합 앵커로 해석)                              |
| `--right-of <LABEL>` | 유일하게 라벨된 셀의 오른쪽 셀을 대상으로 합니다                                 |
| `--below <LABEL>`    | 유일하게 라벨된 셀의 아래쪽 셀을 대상으로 합니다                                 |
| `--text <TEXT>`      | 바꿀 텍스트(빈 문자열은 셀을 비웁니다). 단일 편집에는 필수                       |
| `--map <MAP_JSON>`   | 일괄 편집용 명세 맵(`CellSpec` JSON 배열). 위 플래그들과 함께 쓸 수 없습니다     |
| `-o`, `--output`     | 출력 HWPX 경로(필수)                                                             |

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

| 옵션                  | 설명                                                                                   |
| --------------------- | -------------------------------------------------------------------------------------- |
| `--section <SECTION>` | 섹션 인덱스                                                                            |
| `--anchor <ANCHOR>`   | 기준 문단 인덱스(새 문단이 이 문단의 문단·문자 모양을 그대로 물려받습니다)             |
| `--before`            | 기준 앞에 삽입합니다(기본값은 뒤)                                                      |
| `--text <TEXTS>`      | 새 문단의 한 줄 일반 텍스트. 반복하면 연속된 블록을 한 번의 검증된 편집으로 삽입합니다 |
| `-o`, `--output`      | 출력 HWPX 경로(필수)                                                                   |

섹션의 첫 문단(secPr를 담은 문단) 앞 삽입은 거부됩니다. 왕복 안전한 입력만 편집할 수 있습니다.

### delete-para

최상위 본문 문단을 인덱스로 삭제합니다. 전부 성공하거나 전부 취소됩니다.

```console
hwpforge delete-para [OPTIONS] --section <SECTION> --output <OUTPUT> <FILE>
```

| 옵션                   | 설명                                                        |
| ---------------------- | ----------------------------------------------------------- |
| `--section <SECTION>`  | 섹션 인덱스                                                 |
| `--index <INDICES>...` | 삭제할 최상위 문단 인덱스(일괄 삭제는 반복). 하나 이상 필수 |
| `-o`, `--output`       | 출력 HWPX 경로(필수)                                        |

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

## 0.18 오류 코드와 종료 상태

CLI JSON의 `code`, MCP 도구 오류의 `code`, Python `HwpForgeError.code`는 같은 실패 단계와 도메인 거부 사유에 같은 `OpsCode` 문자열을 사용합니다. 여러 결함이 있으면 실제로 먼저 실패한 검사로 분류하며, 메시지·힌트·응답 봉투까지 같다는 뜻은 아닙니다. 퇴역 별명과 옛 문자열 lookup은 제공하지 않습니다.

| 실패 단계·원인                                           | 코드                                                        |
| -------------------------------------------------------- | ----------------------------------------------------------- |
| 입력 HWPX / Markdown / HWP5 해석                         | `DECODE_FAILED` / `MD_DECODE_FAILED` / `HWP5_DECODE_FAILED` |
| 편집 전 no-op 검증·인코드·재디코드 또는 의미 왕복 불일치 | `INPUT_NOT_ROUNDTRIP_SAFE`                                  |
| 입력 ZIP 엔트리 미보존                                   | `INPUT_ENTRIES_NOT_CARRIED`                                 |
| 명시적 문서 검증·메모리 문서 깊이 검사                   | `VALIDATION_FAILED`                                         |
| 출력 생성 / 의미 손실 때문에 출력 거부                   | `ENCODE_FAILED` / `ENCODE_SEMANTIC_LOSS`                    |
| 생성 산출물 읽기·재디코드·대조                           | `SELF_VERIFY_FAILED`                                        |
| 보호 필드 침범 / 내부 불변식 파손                        | `PROTECTED_FIELD_RANGE` / `INTERNAL_INVARIANT`              |
| 보존 patch의 변경·metadata 거부 / fill 고유 거부         | `PATCH_FAILED` / `FILL_FAILED`                              |
| 아직 분류하지 않은 상류 variant                          | `UPSTREAM_UNMAPPED`                                         |

`INPUT_NOT_ROUNDTRIP_SAFE`는 현재 코덱으로 무변경 왕복을 확인하지 못했다는 뜻입니다. 원문을 보존하는 `to-json --section N` → `patch`, 누름틀 `fill` 경로를 검토하세요. 입력 바이트의 깊이 초과는 `DECODE_FAILED`이고, Rust Draft 셀 편집의 깊이 초과는 `VALIDATION_FAILED`입니다.

종료 상태는 **(명령, 코드)로** 결정합니다. 같은 명령·코드는 오류 payload나 발생 경로에 따라 달라지지 않습니다. 0은 성공이고, `validate`의 3은 디코드된 문서가 유효하지 않다는 판정입니다(Core 중첩 깊이 32 초과는 디코드 거부라 `DECODE_FAILED`입니다). 오류의 1·2만으로 원인을 구분하지 말고 `code`를 확인하세요.

| I/O 코드                             | 종료 상태                                                  |
| ------------------------------------ | ---------------------------------------------------------- |
| `FILE_NOT_FOUND`, `FILE_READ_FAILED` | `to-md`·`convert-hwp5`는 2, 나머지는 1                     |
| `INPUT_TOO_LARGE`                    | 일반 파일 사전 검사·FIFO 모두 1                            |
| `FILE_WRITE_FAILED`                  | `delete-para`·`insert-para`·`convert-hwp5`는 2, 나머지는 1 |

0.18에서 바뀌는 종료 값은 다음 일곱 변경군입니다.

| 명령·실패                                                               |    이전 | 0.18 코드·종료                 |
| ----------------------------------------------------------------------- | ------: | ------------------------------ |
| `delete-para`·`insert-para`: no-op validate·encode·재decode             |       2 | `INPUT_NOT_ROUNDTRIP_SAFE` / 1 |
| `stamp`: no-op validate·encode·재decode                                 |       2 | `INPUT_NOT_ROUNDTRIP_SAFE` / 1 |
| `set-cell`: 입력 decode·원본 패키지 읽기                                |       1 | `DECODE_FAILED` / 2            |
| `delete-para`·`insert-para`: 출력 decode·생성 산출물 읽기·fragment 파싱 |       2 | `SELF_VERIFY_FAILED` / 1       |
| `to-md`·`convert-hwp5`: 크기 한도를 넘는 FIFO 입력                      |       2 | `INPUT_TOO_LARGE` / 1          |
| `convert-hwp5`: 쓰기 성공 직후 stat 실패                                |       1 | `FILE_WRITE_FAILED` / 2        |
| `validate`: 파서는 통과하고 Core 중첩 깊이가 32를 넘는 HWPX             | 3(판정) | `DECODE_FAILED` / 2            |

CLI 전용 `INVALID_SET`·`DUPLICATE_SET`과 진단용 네 코드는 유지합니다. 잘못된 명령행 문법은 기존 파서의 종료 2와 사용법 텍스트를 유지합니다. PDF의 `cause.code`는 별도 렌더러 코드 체계입니다.

이전 코드로 분기하던 소비자는 다음 이행표를 참고하세요. 아래 왼쪽 열은 과거 이름이며 0.18 출력이나 lookup 별명이 아닙니다.

| 이전 코드·용도                                                                                  | 0.18 코드                                                                                                                   |
| ----------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| `DECODE_ERROR`, `HWPX_DECODE_FAILED`                                                            | `DECODE_FAILED`                                                                                                             |
| `ENCODE_ERROR`                                                                                  | `ENCODE_FAILED`                                                                                                             |
| `VALIDATION_ERROR`, `VALIDATE_FAILED`                                                           | `VALIDATION_FAILED`                                                                                                         |
| `MD_DECODE_ERROR`, `JSON_PARSE_ERROR`                                                           | `MD_DECODE_FAILED`, `JSON_PARSE_FAILED`                                                                                     |
| `SERIALIZE_ERROR`, `STAMP_MANIFEST_SERIALIZE`                                                   | `JSON_SERIALIZE_FAILED`                                                                                                     |
| `UNKNOWN_PRESET`                                                                                | `PRESET_NOT_FOUND`                                                                                                          |
| `STYLE_STORE_ERROR`, `STYLE_REBIND_ERROR`                                                       | `STYLE_STORE_FAILED`, `STYLE_REBIND_FAILED`                                                                                 |
| `FILL_ERROR`, 포괄 `FILL_FAILED`                                                                | 고유 fill 거부는 `FILL_FAILED`, 나머지는 실패 단계·patch·불변식 코드                                                        |
| `PATCH_ERROR`, 포괄 `PATCH_FAILED`                                                              | 보존 정책 거부는 `PATCH_FAILED`, 나머지는 실패 단계·보호 필드·불변식·구역 범위 코드                                         |
| `SET_CELL_CODEC_FAILED`, `STAMP_CODEC_FAILED`, `STRUCTURAL_CODEC`                               | 해당 단계 코드, `PROTECTED_FIELD_RANGE`, `INTERNAL_INVARIANT`; 의미 손실은 `ENCODE_SEMANTIC_LOSS`                           |
| `READ_SECTION_OUT_OF_RANGE`, `READ_TABLE_OUT_OF_RANGE`, `READ_FIELD_NOT_FOUND`                  | `SECTION_OUT_OF_RANGE`, `TABLE_NOT_FOUND`, `FIELD_NOT_FOUND`                                                                |
| `STAMP_DELTA_MISMATCH`, `STAMP_CELL_TARGET_DUPLICATE`                                           | `SELF_VERIFY_FAILED`, `CELL_TARGET_DUPLICATE`                                                                               |
| `INDEX_OUT_OF_RANGE`                                                                            | `SECTION_OUT_OF_RANGE` 또는 `PARAGRAPH_OUT_OF_RANGE`                                                                        |
| `UNCARRIED_ZIP_ENTRIES`                                                                         | `INPUT_ENTRIES_NOT_CARRIED`                                                                                                 |
| MCP 앞 삽입 거부의 `SECTION_PROPERTIES_PARAGRAPH`                                               | `INSERT_BEFORE_SECTION_PROPERTIES`; 원래 구역 속성 문단 보호에는 기존 코드 유지                                             |
| `STRUCTURAL_EDIT_FAILED`                                                                        | 정밀 도메인 코드 또는 `UPSTREAM_UNMAPPED`                                                                                   |
| `SECTION_WORKFLOW_FAILED`, `SECTION_WORKFLOW_ERROR`, `SET_CELL_FAILED`, `STAMP_FAILED` fallback | `UPSTREAM_UNMAPPED`                                                                                                         |
| 포괄 `HWP5_CONVERT_FAILED`                                                                      | decode는 `HWP5_DECODE_FAILED`, 직접 검증은 `VALIDATION_FAILED`, 출력 생성은 `ENCODE_FAILED`; layout replay는 기존 코드 유지 |
| `READ_ERROR`, `METADATA_ERROR`, `STDIN_READ_FAILED`                                             | `FILE_READ_FAILED`; 입력 부재는 `FILE_NOT_FOUND`                                                                            |
| `WRITE_ERROR`, `DIR_CREATE_ERROR`, `DIR_CREATE_FAILED`                                          | 출력 I/O는 `FILE_WRITE_FAILED`, 출력 경로 정책은 `INVALID_OUTPUT_PATH`                                                      |

과거 로그의 포괄 코드는 당시 실패 단계를 모르면 새 코드 하나로 자동 변환할 수 없습니다.

MCP의 도구 필드 역직렬화 실패는 필드 경로를 메시지에 포함한 `INVALID_PARAMS`입니다. 경고가 있는 18개 도구는 `{code, message, hint?}` 객체 배열을 반환하며, 빈 경고는 생략합니다. `templates`에는 경고 필드가 없습니다. 전송·JSON-RPC·알 수 없는 도구·worker 실패는 기존 프로토콜 경계를 유지합니다.

Python 파일 I/O는 `HwpForgeError`와 `OSError`로 포착할 수 있고, 입력 부재는 `FileNotFoundError`로도 포착할 수 있습니다. 정확한 타입 비교나 `PermissionError` 같은 세부 포착은 달라집니다. Python 호출 규약·인자 변환의 `TypeError`·`ValueError`는 유지합니다.
