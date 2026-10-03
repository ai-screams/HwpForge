# 결과·경고·오류

## 결과 객체 세 가지

값을 만들어 내는 연산은 값과 보고서를 한 객체로 돌려줍니다. 셋 다 `frozen` dataclass입니다.

| 타입                | 필드                              | 돌려주는 연산                                                                                                    |
| ------------------- | --------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| `DocumentResult[R]` | `document: Document`, `report: R` | `fill`·`set_cell`·`patch`·`insert_para`·`delete_para`·`stamp`·`restyle`, `convert_md`·`from_json`·`convert_hwp5` |
| `TextResult[R]`     | `text: str`, `report: R`          | `to_json`·`export_section`·`to_md`                                                                               |
| `BytesResult[R]`    | `data: bytes`, `report: R`        | `to_pdf`                                                                                                         |

검사 연산(`inspect`·`outline`·`fields`·`validate`·`read`·`diff`·`stamp_plan`)과 `templates`·`schema`는 보고서 `dict`만 돌려줍니다.

```python
import hwpforge

result = hwpforge.convert_md("# 제목\n\n본문")
result.document          # Document
result.report            # {'sections': 1, 'paragraphs': ..., 'assets': [], 'warnings': []}
document, report = result.document, result.report      # 필드 이름으로 풀어 쓰는 편이 안전합니다
```

## 보고서와 타입 힌트

보고서는 `TypedDict`입니다. 키 이름은 연산 계층의 보고서 구조체 필드명과 같고, `hwpforge._hwpforge` 스텁(`_hwpforge.pyi`)에 전부 선언돼 있습니다. 편집기에서 자동 완성과 타입 검사를 받으려면 그 이름을 가져오세요(스텁 모듈 자체는 비공개이지만 타입 이름은 안정적인 편입니다).

```python
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from hwpforge._hwpforge import FillReport, InspectReport

def summarize(report: "InspectReport") -> str:
    return f"{report['sections']} sections, {report['paragraphs']} paragraphs"
```

`NotRequired`로 선언된 키(예: `InspectReport["styles"]`, `StampReport["manifest"]`, 경고의 `hint`)는 없을 수 있으니 `.get()`으로 읽으세요. 그 밖의 키는 항상 있고, 값이 없으면 `None`입니다.

## `warnings` — 조용한 손실을 드러내는 채널

`templates()`·`schema()`를 뺀 모든 보고서에 `warnings: list[{code, message, hint?}]`가 있습니다. 실패가 아니라 "성공했지만 이것은 보존되지 않았다"는 신호이므로, 저장하기 전에 읽는 습관이 필요합니다.

```python
doc = hwpforge.Document.open("hancom.hwpx")
rebuilt = hwpforge.from_json(doc.to_json().text, base=doc)
for warning in rebuilt.report["warnings"]:
    print(warning["code"], "-", warning["message"])
    if warning.get("hint"):
        print("  →", warning["hint"])
# LAYOUT_CACHE_DROPPED - ...
```

자주 보게 되는 코드:

| 코드                   | 뜻                                                                         | 어디서                                 |
| ---------------------- | -------------------------------------------------------------------------- | -------------------------------------- |
| `LAYOUT_CACHE_DROPPED` | 원본의 줄 조판 캐시를 재인코드가 싣지 않았다 — 한컴에서 다시 저장하면 복원 | `from_json`·`restyle` 등 재인코드 경로 |
| `IMAGE_EMBED_SKIPPED`  | Markdown의 이미지를 읽지 못해 뺐다(`base_dir` 없음, 경로 밖, 미지원 형식)  | `convert_md`                           |
| 디코더 경고            | 문서가 가졌지만 모델이 담지 못한 것(예: 알 수 없는 컨트롤)                 | 디코드하는 모든 연산                   |

같은 경고를 `validate()`의 `warnings`로도 볼 수 있으므로, 문서를 처음 받았을 때 한 번 `validate()`를 부르는 것이 좋은 시작입니다.

## `HwpForgeError` — 하나의 예외, 다섯 속성

입력을 받아들인 뒤 연산이 거부하면 `HwpForgeError`를 던집니다. `str(exc)`는 `"CODE: message"`(힌트가 있으면 둘째 줄에 `hint: …`)입니다.

| 속성      | 타입           | 내용                                                                                        |
| --------- | -------------- | ------------------------------------------------------------------------------------------- |
| `code`    | `str`          | 안정된 실패 코드. 분기는 이 문자열로                                                        |
| `message` | `str`          | 무엇이 잘못됐는지 한 문장                                                                   |
| `hint`    | `str \| None`  | 어떻게 해야 하는지 — 연산이 알 때만                                                         |
| `cause`   | `dict \| None` | 두 번째 분류. 현재는 `PDF_RENDER_FAILED`만 `{"stage", "code", "kind"?, "location"?}`를 가짐 |
| `details` | `dict \| None` | 구조화된 페이로드. `ENCODE_SEMANTIC_LOSS`는 `{"warnings": [...], "others": [...]}`          |

```python
try:
    doc.set_cell(table=0, at="9,9", text="값")
except hwpforge.HwpForgeError as exc:
    if exc.code == "CELL_NOT_FOUND":
        ...
    elif exc.code in ("INPUT_ENTRIES_NOT_CARRIED", "INPUT_NOT_ROUNDTRIP_SAFE"):
        ...     # 한컴 저장 문서 — patch 경로로
    else:
        raise
```

`code`는 CLI·MCP와 대체로 같지만 **같은 실패라도 창구마다 동결된 문자열이 다를 수 있습니다**(예: CLI `DECODE_FAILED`와 MCP `DECODE_ERROR`). Python 코드는 이 패키지의 계약이므로 이 문서의 표를 기준으로 삼으세요.

### 다른 예외

| 예외                          | 언제                                                                                          |
| ----------------------------- | --------------------------------------------------------------------------------------------- |
| `TypeError` / `ValueError`    | 인자 변환 실패 — `fill` 값에 정수, `font_dirs`에 문자열 하나, `Document(…)`에 `bytes` 아닌 것 |
| `OSError`                     | `Document.open`·`save`의 파일 I/O                                                             |
| `AttributeError`              | `Document` 속성 대입·삭제(불변)                                                               |
| `pyo3_runtime.PanicException` | 내부 Rust panic — 버그이니 이슈로 알려 주세요                                                 |

## 코드 표

각 연산이 자기 특성 실패로 내는 코드입니다. HWPX 문서를 디코드하는 연산은 디코드 실패를 `DECODE_FAILED`로 냅니다. 다만 `fill`·`set_cell`·`insert_para`·`delete_para`·`stamp`·`stamp_plan`은 디코드 실패도 자기 codec 코드(`FILL_FAILED`·`SET_CELL_CODEC_FAILED`·`STRUCTURAL_CODEC`·`STAMP_CODEC_FAILED`)로 내므로 `DECODE_FAILED`는 나오지 않습니다. 코드는 `hwpforge::foundation::diagnostics::OpsCode`의 문자열 그대로이며, 아래 표는 Python이 부르는 연산에서 실제로 나올 수 있는 것만 적었습니다.

| 연산                           | 코드                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `Document.open`                | `INPUT_TOO_LARGE`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| 그 밖에 HWPX를 디코드하는 연산 | `DECODE_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `read`                         | `READ_TARGET_REQUIRED` · `READ_PARAS_INVALID` · `READ_PARAS_WITHOUT_SECTION` · `READ_SECTION_OUT_OF_RANGE` · `READ_PARA_RANGE_INVALID` · `READ_TABLE_OUT_OF_RANGE` · `READ_FIELD_NOT_FOUND` · `TABLE_GRID_INVALID`                                                                                                                                                                                                                                                                                                                                                                |
| `export_section`               | `SECTION_OUT_OF_RANGE` · `GRID_ADDR_PROJECTION_FAILED` · `JSON_SERIALIZE_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `to_json`                      | `GRID_ADDR_PROJECTION_FAILED` · `JSON_SERIALIZE_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `to_md`                        | `INVALID_INPUT` (알 수 없는 `mode`) · `ENCODE_FAILED` (`mode="lossless"`) · `VALIDATION_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `to_pdf`                       | `INVALID_DISCOVERY` · `UNRECOGNIZED_FORMAT` · `HWP5_DECODE_FAILED` · `HWP5_CONVERT_FAILED` · `VALIDATION_FAILED` · `PDF_RENDER_FAILED` (+ `cause`)                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `fill`                         | `NO_VALUES` · `FIELD_NOT_FOUND` · `FIELD_NAME_AMBIGUOUS` · `FIELD_NOT_FILLABLE` · `EMPTY_FIELD_VALUE` · `FILL_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `set_cell`                     | `INVALID_SET_CELL_ARGS` · `INVALID_SET_CELL_MAP` · `TABLE_NOT_FOUND` · `TABLE_GRID_INVALID` · `CELL_NOT_FOUND` · `CELL_LABEL_AMBIGUOUS` · `CELL_HAS_NON_TEXT_CONTENT` · `CELL_TARGET_DUPLICATE` · `CELL_TARGET_CONFLICT` · `SET_CELL_CODEC_FAILED` · `INPUT_ENTRIES_NOT_CARRIED` · `INPUT_NOT_ROUNDTRIP_SAFE` · `ENCODE_SEMANTIC_LOSS`                                                                                                                                                                                                                                            |
| `patch`                        | `JSON_PARSE_FAILED` · `GRID_ADDR_INVALID` · `SECTION_OUT_OF_RANGE` · `SECTION_INDEX_MISMATCH` · `PATCH_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `insert_para`                  | `INSERT_TEXT_REQUIRED` · `MULTI_PARAGRAPH_TEXT` · `SECTION_OUT_OF_RANGE` · `PARAGRAPH_OUT_OF_RANGE` · `INSERT_BEFORE_SECTION_PROPERTIES` · `STRUCTURAL_CODEC` · `INPUT_ENTRIES_NOT_CARRIED` · `INPUT_NOT_ROUNDTRIP_SAFE` · `SELF_VERIFY_FAILED` · `SPAN_COUNT_MISMATCH`                                                                                                                                                                                                                                                                                                           |
| `delete_para`                  | `DELETE_NO_TARGET` · `SECTION_OUT_OF_RANGE` · `PARAGRAPH_OUT_OF_RANGE` · `DUPLICATE_TARGET` · `REFERENCE_STRANDED` · `HARD_BREAK_LOSS` · `EMPTY_SECTION` · `SECTION_PROPERTIES_PARAGRAPH` · `STRUCTURAL_CODEC` · `INPUT_ENTRIES_NOT_CARRIED` · `INPUT_NOT_ROUNDTRIP_SAFE` · `SELF_VERIFY_FAILED` · `SPAN_COUNT_MISMATCH`                                                                                                                                                                                                                                                          |
| `stamp`                        | `INVALID_STAMP_MAP` · `STAMP_SPEC_STALE` · `STAMP_MARKER_MISMATCH` · `STAMP_SPEC_DUPLICATE` · `STAMP_NAME_EMPTY` · `STAMP_NAME_DUPLICATE` · `STAMP_NAME_COLLISION` · `STAMP_CANDIDATE_UNCOVERED` · `STAMP_CELL_NOT_ANCHOR` · `STAMP_CELL_NOT_EMPTY` · `STAMP_LABEL_DRIFT` · `STAMP_CELL_NOT_CANDIDATE` · `STAMP_CELL_TARGET_DUPLICATE` · `STAMP_SOURCE_HASH_MISMATCH` · `STAMP_MANIFEST_INVARIANT` · `STAMP_DELTA_MISMATCH` · `STAMP_CODEC_FAILED` · `TABLE_NOT_FOUND` · `TABLE_GRID_INVALID` · `INPUT_ENTRIES_NOT_CARRIED` · `INPUT_NOT_ROUNDTRIP_SAFE` · `ENCODE_SEMANTIC_LOSS` |
| `stamp_plan`                   | `STAMP_CODEC_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `restyle`                      | `PRESET_NOT_FOUND` · `NO_FONTS` · `VALIDATION_FAILED` · `ENCODE_FAILED` · `ENCODE_SEMANTIC_LOSS`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `convert_md`                   | `PRESET_NOT_FOUND` · `MD_DECODE_FAILED` · `STYLE_STORE_FAILED` · `STYLE_REBIND_FAILED` · `VALIDATION_FAILED` · `ENCODE_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `from_json`                    | `JSON_PARSE_FAILED` · `GRID_ADDR_INVALID` · `VALIDATION_FAILED` · `ENCODE_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `convert_hwp5`                 | `HWP5_DECODE_FAILED` · `HWP5_CONVERT_FAILED`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `schema`                       | `INVALID_INPUT` (알 수 없는 `kind`)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

`templates`·`schema`를 뺀 대부분의 연산에서 `UPSTREAM_UNMAPPED`(이 버전이 아직 분류하지 못한 상류 오류)와 `INTERNAL_INVARIANT`(라이브러리 불변식 위반 — 버그)가 나올 수 있습니다. 둘 다 이슈로 알려 주세요.

## fail-closed 거부 읽기 — `ENCODE_SEMANTIC_LOSS`

재인코드 편집기(`stamp`·`restyle`)는 인코딩이 의미를 잃을 것 같으면 바이트를 아예 만들지 않고 거부합니다. 무엇이 문제였는지는 `details`에 있습니다.

```python
try:
    doc.restyle(preset="modern")
except hwpforge.HwpForgeError as exc:
    if exc.code == "ENCODE_SEMANTIC_LOSS":
        for w in exc.details["warnings"]:      # 거부를 일으킨 의미 손상
            print(w["code"], w["message"])
        for w in exc.details["others"]:        # 같은 인코딩이 낸 그 밖의 경고
            print("also:", w["code"])
```

## 예외를 직렬화·복제할 때

`HwpForgeError`는 `copy`·`pickle`이 다섯 속성을 그대로 보존하도록 `__reduce__`를 정의합니다. 멀티프로세스 워커에서 예외를 부모로 넘겨도 `code`·`details`가 살아 있습니다.
