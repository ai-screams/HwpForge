# MCP 서버 레퍼런스

`hwpforge-mcp`는 HwpForge의 [MCP](https://modelcontextprotocol.io/) 서버입니다. Claude Code 같은 MCP 지원 AI 도구가 한글 문서를 직접 만들고 읽고 편집할 수 있도록 도구 19개, 리소스 4개, 프롬프트 3개를 노출합니다. 이 장의 이름과 매개변수는 `crates/hwpforge-bindings-mcp/src`의 정의에서 옮겼습니다.

모든 도구는 `{ data, summary, next }` 3층 출력 형식을 씁니다. 파일은 대부분 경로로 주고받으며(예외: `hwpforge_convert`는 `is_file: false`로 Markdown을 인라인으로 받고, `hwpforge_to_json`은 `output_path` 없이 JSON을 인라인으로 돌려주고, `hwpforge_diff`는 `output_path` 없이 보고서를 인라인으로 돌려주며, `hwpforge_from_json`은 JSON을 `structure` 문자열로 받습니다), 실패는 오류 응답(`CallToolResult::error`)으로 돌아옵니다.

## 등록

### npm (권장, Rust 툴체인 불필요)

`npx -y`가 플랫폼에 맞는 바이너리를 내려받습니다.

```console
# 현재 프로젝트에서만
claude mcp add hwpforge -- npx -y @hwpforge/mcp

# 모든 프로젝트에서 (user 범위)
claude mcp add --scope user hwpforge -- npx -y @hwpforge/mcp
```

프로젝트 루트의 `.mcp.json`에 직접 적어도 됩니다.

```json
{
  "mcpServers": {
    "hwpforge": {
      "command": "npx",
      "args": ["-y", "@hwpforge/mcp"]
    }
  }
}
```

Claude Code의 MCP 서버는 `claude mcp add` 또는 `.mcp.json`으로 등록합니다. `.claude/settings.json`에 적어도 MCP 서버로 읽히지 않습니다.

### Cargo

```console
cargo install hwpforge-bindings-mcp
claude mcp add hwpforge hwpforge-mcp
```

crates.io 패키지 이름은 `hwpforge-bindings-mcp`이고 설치되는 바이너리 이름은 `hwpforge-mcp`입니다(`[[bin]] name = "hwpforge-mcp"`).

### npm 패키지 구성

`@hwpforge/mcp`는 플랫폼별 패키지를 optionalDependencies로 가진 기본 패키지입니다. 플랫폼 패키지는 `.github/workflows/npm-publish.yml`의 빌드 매트릭스와 같은 다섯 개입니다.

| 플랫폼 패키지 접미사 | Rust 타깃                   |
| -------------------- | --------------------------- |
| `darwin-arm64`       | `aarch64-apple-darwin`      |
| `darwin-x64`         | `x86_64-apple-darwin`       |
| `linux-x64`          | `x86_64-unknown-linux-gnu`  |
| `linux-arm64`        | `aarch64-unknown-linux-gnu` |
| `win32-x64`          | `x86_64-pc-windows-msvc`    |

패키지 이름은 `@hwpforge/mcp-<접미사>` 형태입니다. 게시는 npm Trusted Publishing(OIDC)만 씁니다.

## 도구

표의 `(.hwpx)`는 그 `output_path`가 `.hwpx`로 끝나야 한다는 표시입니다. 이 확장자 검사는 `hwpforge_convert`·`hwpforge_from_json`·`hwpforge_restyle`·`hwpforge_fill`·`hwpforge_set_cell`·`hwpforge_patch`·`hwpforge_stamp`에 있고, `hwpforge_insert_para`·`hwpforge_delete_para`에는 없습니다. `hwpforge_to_json`의 `output_path`는 `.json`으로 끝나야 합니다.

### 만들기 · 변환

| 도구                 | 용도                                                     | 매개변수                                                                                                                |
| -------------------- | -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| `hwpforge_convert`   | Markdown을 HWPX로 변환합니다                             | `markdown`(파일 경로 또는 인라인 내용), `is_file`(기본 `true`), `output_path`(.hwpx), `preset`(기본 `default`)          |
| `hwpforge_to_json`   | HWPX를 편집용 JSON으로 내보냅니다                        | `file_path`, `section`(선택, 0부터), `output_path`(선택, `.json`으로 끝나야 함, 없으면 JSON을 응답에 인라인으로 돌려줌) |
| `hwpforge_from_json` | JSON(`ExportedDocument` 스키마)으로 HWPX를 직접 만듭니다 | `structure`(JSON 문자열), `output_path`(.hwpx)                                                                          |
| `hwpforge_to_md`     | HWPX를 Markdown으로 변환합니다                           | `file_path`, `output_dir`(선택, 기본값은 입력과 같은 디렉터리)                                                          |
| `hwpforge_restyle`   | 기존 HWPX에 다른 스타일 프리셋을 적용합니다              | `file_path`, `preset`, `output_path`(.hwpx)                                                                             |
| `hwpforge_templates` | 스타일 프리셋 목록을 돌려줍니다                          | `name`(선택, 프리셋 이름 필터)                                                                                          |

`hwpforge_to_json`의 인라인 응답은 직렬화된 응답이 1 MB 미만일 때만 가능하며, 더 큰 내보내기는 `OUTPUT_TOO_LARGE`로 거부되므로 `output_path`를 주세요. 전체 문서 내보내기는 `hwpforge_from_json`이, `section`을 준 내보내기는 `hwpforge_patch`가 읽습니다.

### 읽기 · 검사

| 도구                | 용도                                                                          | 매개변수                                                                                                    |
| ------------------- | ----------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `hwpforge_inspect`  | 섹션·문단·표·이미지·차트·머리글/바닥글·쪽번호 개수와 디코드 경고를 돌려줍니다 | `file_path`, `styles`(예약 필드, 현재 무시됨)                                                               |
| `hwpforge_outline`  | 제목, 표, 이름 있는 누름틀, 책갈피의 내비게이션 맵                            | `file_path`                                                                                                 |
| `hwpforge_fields`   | 누름틀의 이름, 힌트, 현재 값, 채울 수 있는지 여부                             | `file_path`                                                                                                 |
| `hwpforge_read`     | 문단 범위, 표 격자, 필드 중 하나만 텍스트로 읽습니다                          | `file_path`, `section`, `paras`(`"A..B"` 또는 `"N"`), `table`, `field` — section/table/field 중 정확히 하나 |
| `hwpforge_diff`     | 두 HWPX를 semantic·package 두 채널로 비교합니다                               | `base_path`, `revised_path`, `output_path`(선택, 보고서가 인라인 1 MB를 넘으면 필수)                        |
| `hwpforge_validate` | HWPX 구조와 무결성을 검사합니다                                               | `file_path`                                                                                                 |

`hwpforge_validate`는 디코드할 수 없는 파일(예: `.hwp`)을 "유효하지 않은 문서"가 아니라 `DECODE_FAILED` 오류로 알립니다.

### 편집

| 도구                   | 용도                                                                  | 매개변수                                                                                                                                                        |
| ---------------------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `hwpforge_fill`        | 이름 있는 누름틀을 이름→값 맵으로 채웁니다(전부 성공하거나 전부 취소) | `file_path`, `values`(이름→값 맵), `output_path`(.hwpx)                                                                                                         |
| `hwpforge_set_cell`    | 표 셀을 논리 격자 주소로 편집합니다                                   | `file_path`, `specs`(셀 명세 배열: 표 순번 + `at {row,col}` / `right_of` / `below` + `text`), `output_path`(.hwpx)                                              |
| `hwpforge_patch`       | 섹션 하나를 편집한 JSON으로 교체합니다(텍스트 전용)                   | `base_path`, `section`, `section_json_path`, `output_path`(.hwpx)                                                                                               |
| `hwpforge_insert_para` | 기준 문단 앞뒤에 새 최상위 문단을 삽입합니다                          | `file_path`, `section`, `anchor`, `before`(기본 `false`), `text` 또는 `texts` 중 정확히 하나, `output_path`                                                     |
| `hwpforge_delete_para` | 최상위 본문 문단을 인덱스로 삭제합니다                                | `file_path`, `section`, `indices`, `output_path`                                                                                                                |
| `hwpforge_stamp_plan`  | 자리표시자 후보를 찾습니다                                            | `file_path`                                                                                                                                                     |
| `hwpforge_stamp`       | 승인된 명세로 자리표시자를 누름틀로 승격합니다                        | `file_path`, `specs`(텍스트 명세), `cells`(셀 명세), `source_sha256`(`cells`를 쓰면 필수), `output_path`(.hwpx), `manifest_path`(기본 `<output>.manifest.json`) |

편집 도구의 동작 규칙은 [CLI 레퍼런스](./cli.md)의 대응하는 명령(예: `hwpforge_insert_para`는 `insert-para`)과 같습니다. 주의할 점은 다음과 같습니다.

- `hwpforge_patch`는 문단 구조를 바꾸지 못합니다. 의미 텍스트 슬롯의 개수나 경로가 다르면 거부되며, 문단 추가·삭제는 `hwpforge_insert_para`·`hwpforge_delete_para`, 표 셀은 `hwpforge_set_cell`, 구조가 바뀐 문서 재구성은 `hwpforge_from_json`을 씁니다.
- `hwpforge_delete_para`·`hwpforge_insert_para`·`hwpforge_set_cell`·`hwpforge_stamp`는 왕복 안전한 입력만 편집합니다. 인코더가 ZIP 엔트리를 모두 실어 보낼 수 없는 문서, 곧 한컴이 저장하며 `Preview/*`와 `META-INF/container.rdf`를 더한 문서는 거부됩니다. 이런 문서에도 `hwpforge_to_json` + `hwpforge_patch`(텍스트)와 `hwpforge_fill`(누름틀)은 쓸 수 있습니다.
- `hwpforge_stamp`는 `hwpforge_stamp_plan`이 준 후보 객체를 그대로 복사해 `action`(`{"field":{"name":"…"}}` 또는 `"ignore"`)만 더합니다. 지침 문맥의 후보는 자동 적용되지 않습니다.

## 리소스

스타일 템플릿을 YAML(`application/x-yaml`)로 읽는 리소스 4개입니다. 프리셋 이름과 일치합니다(`crates/hwpforge-bindings-mcp/src/resources/mod.rs`).

| URI                            | 이름             |
| ------------------------------ | ---------------- |
| `hwpforge://templates/default` | Default Template |
| `hwpforge://templates/modern`  | Modern Template  |
| `hwpforge://templates/classic` | Classic Template |
| `hwpforge://templates/latest`  | Latest Template  |

## 프롬프트

워크플로 안내 프롬프트 3개입니다(`crates/hwpforge-bindings-mcp/src/prompts/mod.rs`).

| 이름                 | 제목                 | 인자                                                                                              |
| -------------------- | -------------------- | ------------------------------------------------------------------------------------------------- |
| `generate_proposal`  | 정부 제안서 생성     | `topic`(필수), `organization`(선택), `deadline`(선택, YYYY-MM-DD)                                 |
| `generate_report`    | 보고서 생성          | `topic`(필수), `author`(선택), `report_type`(선택: research / progress / analysis, 기본 research) |
| `convert_and_review` | 문서 편집 워크플로우 | `file_path`(필수), `edit_instructions`(선택)                                                      |

## 참고

- MCP 서버는 베타이며 HWP5 경로는 MCP가 아니라 CLI(`convert-hwp5`, `audit-hwp5`, `to-pdf`)를 우선합니다. 이 서버의 19개 도구에는 HWP5와 PDF 관련 도구가 없습니다.
- CLI 명령은 [CLI 레퍼런스](./cli.md), Python 사용법은 [Python 가이드](../guide/python.md)를 보세요.
