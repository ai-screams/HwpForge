# API 요약

패키지의 공개 표면은 `hwpforge.__all__`의 열한 이름입니다: `Document` · `DocumentResult` · `TextResult` · `BytesResult` · `HwpForgeError` · `convert_md` · `from_json` · `convert_hwp5` · `templates` · `schema` · `__version__`. 그 밖의 모듈(`hwpforge._hwpforge` 등)은 비공개이며 릴리스 사이에 바뀔 수 있습니다.

## `Document`

### 생성과 바이트

| 이름                                               | 시그니처                    | 설명                                                                  |
| -------------------------------------------------- | --------------------------- | --------------------------------------------------------------------- |
| `Document.open`                                    | `(path) -> Document`        | 파일에서. 100 MB 상한, 넘으면 `INPUT_TOO_LARGE`. I/O 실패는 `OSError` |
| `Document.from_bytes`                              | `(data: bytes) -> Document` | 메모리의 바이트에서. 상한 없음                                        |
| `Document(data)`                                   | `(data: bytes)`             | 생성자. `bytes` 아니면 `TypeError`                                    |
| `to_bytes`                                         | `() -> bytes`               | HWPX 패키지 바이트 그대로                                             |
| `save`                                             | `(path) -> None`            | 항상 HWPX로 씀. 기존 파일은 교체                                      |
| `bytes(doc)` · `len(doc)` · `==` · `hash` · `repr` |                             | 바이트 기준 값 의미. `repr`은 `Document(<n> bytes)`                   |

### 검사 (보고서 `dict` 반환)

| 메서드       | 시그니처                                                              | 보고서 키                                                                                                     |
| ------------ | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
| `inspect`    | `(*, styles: bool = False) -> InspectReport`                          | `metadata`·`sections`·`paragraphs`·`tables`·`images`·`charts`·`fields`·`section_details`·`styles`?·`warnings` |
| `outline`    | `() -> OutlineReport`                                                 | `outline{title, sections, headings, tables, fields, bookmarks}`·`warnings`                                    |
| `fields`     | `() -> FieldsReport`                                                  | `fields[{name, hint, current, section, fillable}]`·`warnings`                                                 |
| `validate`   | `() -> ValidateReport`                                                | `ok`·`sections`·`paragraphs`·`errors`·`warnings`                                                              |
| `read`       | `(*, section=None, paras=None, table=None, field=None) -> ReadReport` | `paragraphs`·`table`·`fields`(하나만 채워짐)·`warnings`                                                       |
| `diff`       | `(revised: Document) -> DiffReport`                                   | `identical`·`note`·`semantic{…}`·`package{added, removed, changed}`·`warnings`                                |
| `stamp_plan` | `() -> StampPlanReport`                                               | `schema_version`·`source_sha256`·`text`·`cells`·`skipped_tables`·`warnings`                                   |

### 내보내기

| 메서드           | 시그니처                                                                                                          | 반환                                                |
| ---------------- | ----------------------------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `to_json`        | `(*, styles: bool = True) -> TextResult[ToJsonReport]`                                                            | `.text` JSON, `.report{document, warnings}`         |
| `export_section` | `(*, section: int, styles: bool = True) -> TextResult[ExportSectionReport]`                                       | `.text` JSON, `.report{section, warnings}`          |
| `to_md`          | `(*, mode: "styled" \| "lossy" \| "lossless" = "styled") -> TextResult[ToMdReport]`                               | `.text` Markdown, `.report{mode, images, warnings}` |
| `to_pdf`         | `(*, font_dirs=(), discovery="explicit", degraded=False, partial_cache_reject=False) -> BytesResult[ToPdfReport]` | `.data` PDF, `.report{pages, warnings}`             |

### 편집 (`DocumentResult` 반환 — `.document` 새 문서, `.report` 보고서)

| 메서드        | 시그니처                                                                           | 보고서 키                                                                    |
| ------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `fill`        | `(values: Mapping[str, str])`                                                      | `filled[{name, section, previous}]`·`warnings`                               |
| `set_cell`    | `(*, table=None, at=None, right_of=None, below=None, text=None, specs=None)`       | `results[{table, requested, anchor, resolution, cleared}]`·`warnings`        |
| `patch`       | `(*, section: int, patch: str)`                                                    | `section`·`warnings`                                                         |
| `insert_para` | `(*, section: int, anchor: int, text: str \| Sequence[str], before: bool = False)` | `inserted`·`deleted`·`warnings`                                              |
| `delete_para` | `(*, section: int, indexes: Sequence[int])`                                        | `inserted`·`deleted`·`warnings`                                              |
| `stamp`       | `(request: StampRequest, *, manifest: bool = True)`                                | `manifest`?·`stamped`·`stamped_cells`·`ignored`·`skipped_guarded`·`warnings` |
| `restyle`     | `(*, preset: str)`                                                                 | `preset`·`sections`·`paragraphs`·`warnings`                                  |

## 모듈 함수

| 함수           | 시그니처                                                                          | 반환                                                                            |
| -------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `convert_md`   | `(text: str, *, preset: str = "default", base_dir=None)`                          | `DocumentResult[ConvertMdReport]` — `sections`·`paragraphs`·`assets`·`warnings` |
| `from_json`    | `(text: str, *, base: Document \| None = None)`                                   | `DocumentResult[EncodeReport]` — `paragraphs`·`warnings`                        |
| `convert_hwp5` | `(data: bytes, *, carry_layout_cache: bool = False)`                              | `DocumentResult[ConvertHwp5Report]` — `warnings`                                |
| `templates`    | `()`                                                                              | `TemplatesReport` — `presets[{name, description, font, page_size}]`             |
| `schema`       | `(*, kind: "document" \| "exported-document" \| "exported-section" = "document")` | JSON Schema `dict`                                                              |

## 결과 객체와 예외

| 타입                | 필드/속성                                                                                        |
| ------------------- | ------------------------------------------------------------------------------------------------ |
| `DocumentResult[R]` | `document: Document`, `report: R`                                                                |
| `TextResult[R]`     | `text: str`, `report: R`                                                                         |
| `BytesResult[R]`    | `data: bytes`, `report: R`                                                                       |
| `HwpForgeError`     | `code: str`, `message: str`, `hint: str \| None`, `cause: dict \| None`, `details: dict \| None` |

## 입력 형태

| 이름               | 모양                                                                                                                           |
| ------------------ | ------------------------------------------------------------------------------------------------------------------------------ |
| `CellSpec`         | `{"table": int, "text": str}` + `at: {"row", "col"}` \| `right_of: str` \| `below: str` 중 하나                                |
| `StampSpec`        | `{"section", "path", "span": {"start", "end"}, "marker", "action"}` — `action`은 `"ignore"` 또는 `{"field": {"name", "hint"}}` |
| `StampRequest`     | `Sequence[StampSpec]` 또는 `{"schema_version", "source_sha256", "text"?: [...], "cells"?: [...]}`                              |
| `paras`            | `"시작..끝"`, 양끝 포함                                                                                                        |
| `at` (메서드 인자) | `"row,col"` 문자열, 0부터                                                                                                      |

## 상수

| 이름                               | 값                                         |
| ---------------------------------- | ------------------------------------------ |
| `hwpforge.__version__`             | 설치된 패키지 버전                         |
| `hwpforge._hwpforge.MAX_FILE_SIZE` | `Document.open`의 상한, 104857600 (100 MB) |
