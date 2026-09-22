# Python

`hwpforge` 패키지는 HwpForge Rust 라이브러리를 얇게 감싼 Python 바인딩입니다. HWPX 문서를 읽고, 검사하고, 편집하고, Markdown·JSON·PDF 로 내보내며, 옛 HWP5(`.hwp`)를 HWPX 로 변환합니다. 연산의 의미는 CLI·MCP 서버와 같은 연산 계층(`hwpforge::ops`)에서 오므로 세 창구가 같은 문서에 같은 결과를 내고, 메서드 이름·인자 표기·일부 실패 코드만 Python 의 공개 계약으로 따로 정해져 있습니다.

이 페이지는 설치와 핵심 개념을 다루고, 세부는 하위 페이지로 나뉩니다.

| 페이지                                         | 내용                                                                                                             |
| ---------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| [문서 읽기와 검사](python/reading.md)          | `open`·`inspect`·`outline`·`fields`·`read`·`validate`·`diff`·`stamp_plan`                                        |
| [편집](python/editing.md)                      | `fill`·`patch`·`set_cell`·`insert_para`/`delete_para`·`stamp`·`restyle`, 한컴 저장 문서에서 되는 것과 안 되는 것 |
| [변환과 내보내기](python/converting.md)        | `convert_md`·`from_json`·`convert_hwp5`·`to_md`·`to_json`·`to_pdf`                                               |
| [결과·경고·오류](python/results-and-errors.md) | 결과 객체, 보고서의 `warnings`, `HwpForgeError` 의 다섯 속성, 코드 표                                            |
| [레시피](python/recipes.md)                    | 양식 채우기 파이프라인, 표를 CSV 로, 배치 처리, HWP → PDF, pip 없는 호스트                                       |
| [API 요약](python/api.md)                      | 모든 메서드·함수의 시그니처와 반환형 한 표                                                                       |

## 설치

```console
pip install hwpforge
```

[uv](https://docs.astral.sh/uv/)를 쓴다면 `uv add hwpforge`(프로젝트) 또는 `uv pip install hwpforge`(환경)를 사용하세요.

정확 핀(`==X.Y.Z`)보다 `~=X.Y.Z`를 권장합니다. Python 전용 수정은 `X.Y.Z.N` 형태로 나가는데, 정확 핀은 그 수정을 받지 못합니다.

CPython 3.9+를 커버하는 `abi3` wheel을 다섯 플랫폼(Linux manylinux_2_28 x86_64/aarch64, macOS 11+ arm64, macOS 10.12+ x86_64, Windows x64)에 배포하며, sdist(소스 배포)는 Rust 1.92+와 maturin이 필요합니다. 런타임 의존성은 0개입니다. 자유 스레드(free-threaded) 빌드는 stable ABI 가 다루지 않아 지원하지 않습니다.

### pip 없는 호스트

wheel은 zip 파일이고 런타임 의존성이 없으므로, pip 없이 풀어서 바로 import할 수 있습니다. 기준은 배포판 버전이 아니라 glibc 2.28 이상입니다 — CPython 3.9+와 glibc 2.28+를 갖춘 Linux x86_64 호스트라면(예: Debian 10/11/12, Ubuntu 20.04 이상; 아래 파일명은 x86_64 wheel):

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

wheel 파일 자체는 PyPI의 [files 페이지](https://pypi.org/project/hwpforge/#files)에서 받을 수 있습니다 — `pip`이 설치할 때 받는 것과 같은 바이트·sha256입니다. GitHub Release는 발행 후 에셋을 붙일 수 없는 불변 객체라, 별도로 첨부되지 않습니다. 오프라인 배포의 전체 절차는 [레시피](python/recipes.md#pip-없는-호스트에-배포하기)에 있습니다.

## 30초 예제

```python
import hwpforge

doc = hwpforge.Document.open("form.hwpx")
for field in doc.fields()["fields"]:
    print(field["name"], "→", field["current"])

result = doc.fill({"user_email": "kim@example.com"})
for warning in result.report["warnings"]:
    print(warning["code"], warning["message"])
result.document.save("form-filled.hwpx")
```

`doc` 은 그대로이고, 채워진 문서는 `result.document` 입니다. 이 재대입 구조가 이 패키지의 전부라고 해도 지나치지 않습니다 — 아래 네 개념이 그 이유입니다.

## 핵심 개념 네 가지

**1. `Document` 는 불변 값입니다.** HWPX 패키지의 바이트를 들고 있는 값 객체로, 두 문서는 바이트가 같으면 같고(`==`, `hash`), 어떤 메서드도 받은 문서를 바꾸지 않습니다. 속성 대입은 `AttributeError` 입니다. 편집 메서드는 새 문서를 돌려주므로, 결과를 변수에 다시 받아야 합니다.

**2. 연산은 값과 보고서를 함께 돌려줍니다.** 편집은 `DocumentResult(document, report)`, 텍스트 내보내기는 `TextResult(text, report)`, 바이트 내보내기는 `BytesResult(data, report)` 입니다. 셋 다 frozen dataclass 라 보고서를 따로 조회할 필요도, 잃어버릴 일도 없습니다. 검사 연산(`inspect`·`outline` 등)은 보고서(`dict`)만 돌려줍니다.

**3. 보고서의 `warnings` 는 항상 있고, 저장 전에 봐야 합니다.** 실패하지 않은 연산도 조용히 성공하지 않습니다. `templates()`·`schema()` 를 뺀 모든 보고서에 `warnings: [{code, message, hint?}]` 가 있으며, 비어 있을 수는 있어도 빠지지는 않습니다. 문서가 완전히 보존되지 않은 곳(예: 재인코드가 조판 캐시를 버림)을 여기서 알려줍니다.

**4. 실패는 `HwpForgeError` 하나이고, `code` 로 분기합니다.** 입력을 받아들인 뒤 연산이 거부하면 `HwpForgeError` 를 던지며 `code`·`message`·`hint`·`cause`·`details` 다섯 속성을 가집니다. 인자 형이 틀리면 `TypeError`/`ValueError`, 파일 I/O 는 `OSError` 입니다. 예외 메시지 문자열이 아니라 `exc.code` 문자열로 분기하세요.

```python
try:
    doc = doc.set_cell(table=0, at="0,1", text="값").document
except hwpforge.HwpForgeError as exc:
    print(exc.code)      # 예: "INPUT_ENTRIES_NOT_CARRIED"
    print(exc.hint)      # 연산이 아는 되는 길, 없으면 None
```

## 입력 크기 상한

`Document.open` 은 파일을 읽을 때 CLI·MCP 와 같은 상한(`hwpforge._hwpforge.MAX_FILE_SIZE`, 100 MB)을 적용하고, 넘으면 `HwpForgeError(INPUT_TOO_LARGE)` 를 던집니다. 상한은 파일이 보고하는 크기가 아니라 **읽은 바이트 수**에 걸리므로, `stat()` 크기가 0 으로 보이는 FIFO 나 프로세스 치환으로도 더 큰 입력을 밀어 넣을 수 없습니다. `Document.from_bytes` 에는 상한이 없습니다 — 호출자가 이미 바이트를 들고 있어 더 제한할 읽기가 남아 있지 않기 때문입니다. 100 MB 보다 큰 문서를 다뤄야 한다면 직접 읽어서 `from_bytes` 로 넘기면 됩니다.

## 포맷은 한 방향입니다

읽기는 `.hwpx`, 그리고 `hwpforge.convert_hwp5` 를 거친 `.hwp`(HWP5) 입니다. 변환은 HWPX 로 가는 한 방향이라 원본 `.hwp` 로 되돌아가는 길은 없습니다. 쓰기는 `.hwpx` 뿐이며(`save` 는 무엇을 읽었든 HWPX 패키지를 씁니다), 한컴 오피스는 `.hwpx` 를 그대로 엽니다.

## 예제에 쓰인 파일

하위 페이지의 예제는 저장소의 테스트 픽스처를 다음 이름으로 복사해 둔 것을 전제합니다. 같은 이름으로 준비하면 모든 코드 블록이 적힌 그대로 실행됩니다.

| 예제 파일       | 원본 (`tests/fixtures/…`)          | 특징                                        |
| --------------- | ---------------------------------- | ------------------------------------------- |
| `form.hwpx`     | `fields/clickhere_named.hwpx`      | 누름틀 `user_email` 하나                    |
| `grid.hwpx`     | `tables/merged_grid_form.hwpx`     | 3×2 표(라벨 `성명`·`비고`), HwpForge 생성본 |
| `template.hwpx` | `stamp/placeholder_basic.hwpx`     | 스탬프 후보 `(   )`·`□`                     |
| `plain.hwpx`    | `structural/plain_paragraphs.hwpx` | 문단 4개, HwpForge 생성본                   |
| `hancom.hwpx`   | `tables/table_01_basic_2x2.hwpx`   | 한컴 오피스가 저장한 문서                   |
| `old.hwp`       | `structural/plain_inserted.hwp`    | HWP5 바이너리                               |

## 다음 단계

- 패키지 README: [crates/hwpforge-bindings-py/README.md](https://github.com/ai-screams/HwpForge/blob/main/crates/hwpforge-bindings-py/README.md) — 플랫폼별 wheel 표
- [아키텍처 개요](../getting-started/architecture.md) — Python 이 CLI·MCP 와 공유하는 연산 계층
