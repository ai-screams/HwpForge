# Python

`hwpforge` 패키지는 HwpForge Rust 라이브러리를 얇게 감싼 Python 바인딩입니다. 핵심 문서 연산의 의미는 CLI(Hammer)·MCP(Anvil)와 같은 연산 계층에서 오지만, 호출 이름·옵션 표기·호환 에러 코드는 각 바인딩의 공개 계약에 따라 다를 수 있습니다.

## 설치

```console
pip install hwpforge
```

[uv](https://docs.astral.sh/uv/)를 쓴다면 `uv add hwpforge`(프로젝트) 또는 `uv pip install hwpforge`(환경)를 사용하세요.

정확 핀(`==X.Y.Z`)보다 `~=X.Y.Z`를 권장합니다. Python 전용 수정은 `X.Y.Z.N` 형태로 나가는데, 정확 핀은 그 수정을 받지 못합니다.

CPython 3.9+를 커버하는 `abi3` wheel을 다섯 플랫폼(Linux manylinux_2_28 x86_64/aarch64, macOS 11+ arm64, macOS 10.12+ x86_64, Windows x64)에 배포하며, sdist(소스 배포)는 Rust 1.92+와 maturin이 필요합니다. 런타임 의존성은 0개입니다.

### pip 없는 호스트

wheel은 zip 파일이고 런타임 의존성이 없으므로, pip 없이 풀어서 바로 import할 수 있습니다. 기준은 배포판 버전이 아니라 glibc 2.28 이상입니다 — CPython 3.9+와 glibc 2.28+를 갖춘 Linux x86_64 호스트라면(예: Debian 10/11/12, Ubuntu 20.04 이상; 아래 파일명은 x86_64 wheel):

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

wheel 파일 자체는 PyPI의 [files 페이지](https://pypi.org/project/hwpforge/#files)에서 받을 수 있습니다 — `pip`이 설치할 때 받는 것과 같은 바이트·sha256입니다. GitHub Release는 발행 후 에셋을 붙일 수 없는 불변 객체라, 별도로 첨부되지 않습니다.

## 빠른 예제

```python
import hwpforge

doc = hwpforge.Document.open("proposal.hwpx")
print(doc.outline()["outline"]["title"])
doc = doc.fill({"applicant": "홍길동", "date": "2026-09-17"}).document
doc.save("proposal-filled.hwpx")
```

`Document`는 불변 값입니다. `fill`을 비롯한 모든 편집 메서드는 원본을 바꾸지 않고, 새 문서와 그 연산의 보고서를 담은 `DocumentResult`를 반환합니다. 텍스트·바이트 내보내기는 각각 `TextResult`·`BytesResult`를 반환합니다 — 위 예제의 재대입(`doc = doc.fill(...).document`)은 생략할 수 없습니다.

## 입력 크기 상한

`Document.open`은 파일을 읽을 때 CLI·MCP와 같은 상한(`_hwpforge.MAX_FILE_SIZE`, 100 MB)을 적용하고, 넘으면 `HwpForgeError(INPUT_TOO_LARGE)`를 던집니다. 상한은 파일이 보고하는 크기가 아니라 **읽은 바이트 수**에 걸리므로, `stat()` 크기가 0으로 보이는 FIFO나 프로세스 치환으로도 더 큰 입력을 밀어 넣을 수 없습니다.

`Document.from_bytes`에는 상한이 없습니다. 호출자가 이미 바이트를 들고 있어 더 제한할 읽기가 남아 있지 않기 때문입니다. 100 MB보다 큰 문서를 다뤄야 한다면 직접 읽어서 `from_bytes`로 넘기면 됩니다.

## 편집 표면과 한컴 저장 문서

한컴이 저장한 `.hwpx`에는 HwpForge 인코더가 재현하지 않는 패키지 엔트리가 들어 있습니다 — `Preview/PrvText.txt`·`Preview/PrvImage.png`·`META-INF/container.rdf`. 패키지 전체를 다시 인코딩하는 네 연산은 이런 문서를 조용히 손상시키는 대신 **fail-closed로 거부**합니다.

| 연산                                           | 한컴 저장 문서 | 결과                                                        |
| ---------------------------------------------- | -------------- | ----------------------------------------------------------- |
| `fill`                                         | 동작           | 누름틀만 쓰고 나머지 엔트리는 바이트 그대로 보존            |
| `patch`                                        | 동작           | 기존 문단·셀의 텍스트만 바꾸고 패키지를 보존                |
| `set_cell`·`insert_para`·`delete_para`·`stamp` | 거부           | `INPUT_ENTRIES_NOT_CARRIED` 또는 `INPUT_NOT_ROUNDTRIP_SAFE` |
| `from_json(base=...)`                          | 부분 승계      | 이미지만 승계하고 위 엔트리는 버려집니다(별도 경고 없음)    |

따라서 실제 정부 서식을 다룰 때 통하는 길은 둘입니다: 누름틀은 `fill`, 그 밖의 텍스트는 `export_section` → 편집 → `patch`. 구조를 정말 바꿔야 할 때만 `from_json(base=...)`을 쓰고, 결과를 한컴에서 열어 확인하세요.

## `inspect()`가 세는 것 — 접두사가 곧 범위

`inspect()["section_details"][i]`의 키는 접두사로 **무엇을 셌는지**를 말합니다. 네 집합은 서로 포함 관계가 아니라 **비교 불가한 재귀 범위**입니다 — 어느 행도 다른 행의 넓은 판본이 아닙니다.

| 접두사        | 재귀해 들어가는 곳                          | master page | 캡션   |
| ------------- | ------------------------------------------- | ----------- | ------ |
| `top_level_`  | 없음 — 섹션 본문 흐름 자체만                | 아니오      | 아니오 |
| (접두사 없음) | 셀·글상자·각주/미주·메모·머리말/꼬리말      | 예          | 아니오 |
| `deep_`       | 머리말/꼬리말·셀·글상자/주석/도형/메모      | 아니오      | 아니오 |
| `all_`        | 섹션 XML이 담은 전부(묶음 객체의 자식 포함) | 아니오      | 예     |

`top_level_`과 접두사 없는 키는 문단·표·이미지·차트를 세고, `deep_`은 문단만, `all_`은 여섯 가지 객체 수를 셉니다. `non_empty_` 중위사는 세는 집합을 바꾸지 않고 "보이는 텍스트가 있는 문단"으로만 좁히므로, `top_level_non_empty_paragraphs`는 `top_level_paragraphs`를 넘지 않습니다. `all_charts`는 없습니다 — 디코더가 섹션 최상위 문단 밖의 차트를 복원하지 못해, 그런 키를 두면 정작 필요한 문서에서 조용히 과소 계수하게 됩니다.

`all_` 키는 **디코드된 객체**를 세지, 원시 XML 요소를 세지 않습니다. CLI `--json`에는 같은 범위를 예전 철자(`tables`·`images`·`text_boxes` 등)로 내보내는 키가 있는데 그쪽은 원시 스캔이라, 디코더가 표현하지 못하는 요소를 받아들인 문서에서는 더 큰 값이 나올 수 있습니다. 두 값은 교환 가능하지 않습니다. 문단 키에는 이 단서가 붙지 않습니다.

## `validate()`는 두 가지 실패를 구분합니다

문서가 **디코드는 되지만** 모델 불변조건을 어기면 `validate()`는 예외 없이 `ok`가 거짓인 보고서를 돌려주고, 그 이유는 보고서의 `errors` 배열에 담깁니다. 반면 **디코드 자체가 안 되는** 입력은 연산 실패이므로 `HwpForgeError(DECODE_FAILED)`를 던집니다. 즉 "유효하지 않은 문서"는 반환값으로, "문서가 아닌 바이트"는 예외로 옵니다.

## 에러 처리

입력을 받아들인 뒤 연산이 실패하면 `HwpForgeError`를 던지며, 이 예외는 다섯 개 속성을 가집니다. 인자 변환 실패는 `TypeError`/`ValueError`, `Document.open`·`save`의 파일 I/O 실패는 `OSError`, 내부 Rust panic은 `pyo3_runtime.PanicException`입니다.

- `code` — Python 연산 계층의 안정된 실패 코드 문자열(예: `"DECODE_FAILED"`, `"ENCODE_SEMANTIC_LOSS"`). 같은 실패라도 CLI·MCP는 각자 동결된 레거시 문자열을 유지할 수 있습니다.
- `message` — 무엇이 잘못됐는지 한 문장.
- `hint` — 연산이 알고 있으면 어떻게 해야 하는지. 모르면 `None`.
- `cause` — 실패가 두 번째로 더 좁게 분류될 때의 값(현재는 PDF 렌더링만 해당). `{"stage", "code", "kind", "location"}` 형태이고, `kind`·`location`은 없을 수 있습니다. 그 밖의 실패는 `None`.
- `details` — 실패 종류가 구조화된 페이로드를 가질 때의 값. 예를 들어 `"ENCODE_SEMANTIC_LOSS"`(의미 손상 fail-closed 거부)는 `{"warnings": [...], "others": [...]}`를 담습니다 — `warnings`는 거부를 유발한 의미 손상 경고, `others`는 같은 인코딩이 낸 나머지 경고입니다. 그 밖의 실패는 `None`.

```python
import hwpforge

try:
    doc = doc.patch(section=0, patch=modified_json)
except hwpforge.HwpForgeError as exc:
    print(exc.code, exc.message)
    if exc.hint:
        print("hint:", exc.hint)
    if exc.details:
        print("details:", exc.details)
```

## 경고는 항상 먼저 확인하세요

실패하지 않은 연산도 조용히 성공하지 않습니다. `Document`의 18개 문서 연산과 `convert_md`·`convert_hwp5`·`from_json`이 반환하는 보고서(report)에는 `warnings` 키가 항상 존재하며(비어 있을 수는 있어도 빠지지는 않습니다), 문서가 완전히 보존되지 않은 부분을 알려줍니다. `templates()`와 `schema()`의 결과에는 `warnings`가 없습니다. 결과를 저장하기 전에 `result.report["warnings"]`를 먼저 확인하는 습관을 들이세요.

```python
result = doc.fill({"applicant": "홍길동"})
for warning in result.report["warnings"]:
    print(warning["code"], warning["message"])
```

## 다음 단계

- 패키지 README: [crates/hwpforge-bindings-py/README.md](https://github.com/ai-screams/HwpForge/blob/main/crates/hwpforge-bindings-py/README.md) — 전체 op 목록과 플랫폼별 wheel 표
- [아키텍처 개요](../getting-started/architecture.md) — Python이 CLI·MCP와 공유하는 연산 계층
