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
