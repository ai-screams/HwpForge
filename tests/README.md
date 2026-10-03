# Tests Fixture Warehouse

`tests/` is primarily a shared fixture warehouse.
It is **not** the main Rust test crate layout.

## What lives here

```text
tests/
├── README.md
└── fixtures/
    ├── charts/
    ├── fields/
    ├── hwp5/
    ├── images/
    ├── layout/
    ├── mixed/
    ├── pagectl/
    ├── pdf-rules/
    ├── shapes/
    ├── stamp/
    ├── structural/
    ├── tables/
    └── user_samples/
        ├── lists/
        ├── numbering/
        ├── pages/
        ├── tables/
        ├── tabs/
        ├── text/
        └── sample-*.hwp / sample-*.hwpx (feature별 연구 fixture)
```

| 디렉터리        | 내용 (참조하는 테스트로 확인한 용도)                                                                                                                                                             |
| --------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `charts/`       | 차트가 든 `.hwp`/`.hwpx` (CLI 통합 테스트, HWP5 차트 OLE 디코더 테스트)                                                                                                                          |
| `fields/`       | 누름틀·날짜 필드 `.hwpx` (`fields`·`read`·`fill` 계열 테스트)                                                                                                                                    |
| `hwp5/`         | HWP5 `.hwp` 샘플 (`convert-hwp5` CLI 테스트, convert·smithy-hwp5 디코더 테스트). `crossref/`는 교차 참조(책갈피·각주·미주·캡션·개요) 대상 샘플로 convert 테스트가 참조                           |
| `images/`       | 이미지 배치(인라인·셀·앵커)와 PDF 렌더 대조용 `.hwp`/`.hwpx`/`.pdf`, `*_oracle.json`                                                                                                             |
| `layout/`       | 다단·줄 조판 캐시 관련 `.hwp`/`.hwpx` (오래된 줄 조판 캐시 `stale-line-cache.hwpx`는 MCP·ops 테스트가 참조)                                                                                      |
| `mixed/`        | 이미지·차트·머리글/바닥글·글상자 혼합 문서 (CLI 통합 테스트의 `convert-hwp5`·머리글/글상자 이미지 검사, convert 테스트, MCP `inspect` 테스트, `hwpforge` 크레이트의 `ops_export_section` 테스트) |
| `pagectl/`      | 쪽 번호 새로 시작·쪽 감추기 `.hwp` (CLI 통합 테스트)                                                                                                                                             |
| `pdf-rules/`    | PDF 렌더 규칙 검증용 `.hwp`/`.hwpx`/`.pdf` (convert `to_pdf` 테스트, smithy-hwpx 생성기 예제)                                                                                                    |
| `shapes/`       | 도형·글상자 `.hwp`/`.hwpx` (`textbox.hwpx`는 기본 스타일 추출 근거로 코드 주석에 인용됨)                                                                                                         |
| `stamp/`        | 스탬핑용 자리표시자 문서 `placeholder_basic.hwpx`                                                                                                                                                |
| `structural/`   | 문단 삽입·삭제 같은 구조 편집 테스트 입력 `.hwpx`/`.hwp`                                                                                                                                         |
| `tables/`       | 표(병합·중첩 등) `.hwp`/`.hwpx` (CLI·MCP·HWP5 감사 테스트)                                                                                                                                       |
| `user_samples/` | 한컴에서 직접 만든 연구 fixture. `lists`·`numbering`·`pages`·`tables`·`tabs`·`text` 하위 디렉터리와 루트의 `sample-*` 파일                                                                       |

## What does _not_ live here

- the main unit/integration test source files
- a runnable `tests/` crate hierarchy
- arbitrary local conversion outputs that should have stayed in `temp/`

Actual tests run from:

- `crates/*/src/**` inline tests
- `crates/*/tests/*.rs` integration tests (Python 바인딩은 `crates/hwpforge-bindings-py/tests/`의 pytest)
- some crate `examples/*.rs` used as verification helpers

## Working rules

- before deleting a fixture, check whether code references it directly
- fixture filenames are hints, not truth; trust code/tests/parity checks first
- local repro artifacts should not accumulate here unless they are promoted into tracked regression inputs
