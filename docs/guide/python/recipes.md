# 레시피

## 양식 채우기 파이프라인

정부 서식처럼 한컴이 저장한 양식을 값으로 채워 저장하는 가장 흔한 흐름입니다. 누름틀은 `fill`, 나머지는 검증·저장.

```python
import hwpforge

def fill_form(src: str, dst: str, values: dict[str, str]) -> list[str]:
    doc = hwpforge.Document.open(src)

    available = {f["name"] for f in doc.fields()["fields"] if f["fillable"]}
    unknown = set(values) - available
    if unknown:
        raise ValueError(f"양식에 없는 필드: {sorted(unknown)}")

    result = doc.fill(values)
    problems = [f"{w['code']}: {w['message']}" for w in result.report["warnings"]]

    check = result.document.validate()
    if not check["ok"]:
        problems += [f"{e['code']}: {e['message']}" for e in check["errors"]]

    result.document.save(dst)
    return problems

print(fill_form("form.hwpx", "form-filled.hwpx", {"user_email": "kim@example.com"}))
```

- 필드 이름을 미리 대조하면 `FIELD_NOT_FOUND` 를 예외가 아니라 목록으로 다룰 수 있습니다.
- 값이 비어 있으면 `EMPTY_FIELD_VALUE` 입니다. 지우고 싶은 필드는 값 목록에서 빼세요.
- `fill` 은 한컴 저장 문서를 보존하므로 결과를 한컴에서 열면 그대로 보입니다. 재인코드가 없어 조판 캐시도 건드린 문단만 무효화됩니다.

## 표를 읽어 CSV 로

```python
import csv
import hwpforge

def tables_to_csv(path: str, out_prefix: str) -> int:
    doc = hwpforge.Document.open(path)
    tables = doc.outline()["outline"]["tables"]
    for t in tables:
        view = doc.read(table=t["ordinal"])["table"]
        grid = [[""] * view["cols"] for _ in range(view["rows"])]
        for cell in view["cells"]:
            grid[cell["row"]][cell["col"]] = cell["text"]     # 병합 영역은 기준 셀에만 텍스트가 있습니다
        with open(f"{out_prefix}-{t['ordinal']}.csv", "w", newline="", encoding="utf-8") as f:
            csv.writer(f).writerows(grid)
    return len(tables)

print(tables_to_csv("grid.hwpx", "grid"))
```

## 텍스트만 바꾸기 — 한컴 저장 문서에서도 되는 길

```python
import json
import hwpforge

def replace_text(doc: hwpforge.Document, section: int, old: str, new: str) -> hwpforge.Document:
    exported = doc.export_section(section=section)
    payload = json.loads(exported.text)

    def walk(node):
        if isinstance(node, dict):
            if isinstance(node.get("text"), str) and old in node["text"]:
                node["text"] = node["text"].replace(old, new)
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    walk(payload["section"])
    return doc.patch(section=section, patch=json.dumps(payload, ensure_ascii=False)).document

doc = hwpforge.Document.open("hancom.hwpx")
changed = replace_text(doc, 0, "표", "테이블")
print(doc.diff(changed)["package"]["changed"])
```

`patch` 는 텍스트 슬롯만 치환하므로 문단 수·표 구조·서식 참조는 건드리면 안 됩니다. 위 함수처럼 `text` 값만 바꾸면 `PATCH_FAILED` 가 나지 않습니다.

## 디렉터리 배치 처리

```python
from pathlib import Path
import hwpforge

def inventory(root: str) -> list[dict]:
    rows = []
    for path in sorted(Path(root).glob("*.hwpx")):
        doc = hwpforge.Document.open(path)
        try:
            report = doc.inspect()
        except hwpforge.HwpForgeError as exc:
            rows.append({"file": path.name, "error": exc.code})
            continue
        rows.append({
            "file": path.name,
            "title": report["metadata"]["title"],
            "paragraphs": report["paragraphs"],
            "tables": report["tables"],
            "fields": len(report["fields"]),
            "warnings": len(report["warnings"]),
        })
    return rows

for row in inventory("."):
    print(row)
```

문서는 `open` 시점에 디코드되지 않으므로 파일마다 `inspect` 에서 실패를 잡으면 됩니다. CPU 를 더 쓰고 싶으면 `concurrent.futures.ProcessPoolExecutor` 로 파일 단위로 나누세요 — `HwpForgeError` 는 pickle 이 되므로 워커의 실패가 `code` 째로 부모에 돌아옵니다.

## HWP 를 PDF 로

옛 `.hwp` 를 한컴이 계산한 쪽 나눔 그대로 PDF 로 만드는 길입니다. 글꼴 디렉터리는 문서가 이름 붙인 face 가 실제로 있는 곳이어야 합니다.

```python
import hwpforge

def hwp_to_pdf(src: str, dst: str, font_dirs: list[str]) -> int:
    with open(src, "rb") as handle:
        converted = hwpforge.convert_hwp5(handle.read(), carry_layout_cache=True)
    pdf = converted.document.to_pdf(font_dirs=font_dirs)
    with open(dst, "wb") as out:
        out.write(pdf.data)
    return pdf.report["pages"]

try:
    print(hwp_to_pdf("old.hwp", "old.pdf", ["/Library/Fonts/Hancom"]))
except hwpforge.HwpForgeError as exc:
    print(exc.code, exc.cause and exc.cause["code"])     # FONT_UNRESOLVED 면 font_dirs 를 확인
```

글꼴을 구할 수 없고 모양이 달라져도 괜찮다면 `to_pdf(font_dirs=[...], degraded=True)` 로 대체 글꼴 렌더를 받을 수 있습니다. `carry_layout_cache=True` 로 변환한 HWPX 는 PDF 용이며 한컴에서 다시 열어 편집할 문서로 쓰지 마세요.

## Markdown 으로 문서 생성 후 검토

```python
import hwpforge

result = hwpforge.convert_md(open("report.md", encoding="utf-8").read(), preset="modern", base_dir=".")
for asset in result.report["assets"]:
    if asset["kind"] == "dropped":
        print("이미지 누락:", asset["occurrence"], asset["reason"])
result.document.save("report.hwpx")

back = result.document.to_md(mode="lossy")
print(back.text[:300])
```

생성한 문서는 조판 캐시가 없으므로 `to_pdf` 는 되지 않습니다. 한컴에서 열어 저장하면 캐시가 생기고 그때부터 렌더됩니다.

## pip 없는 호스트에 배포하기

NAS 나 잠긴 서버처럼 pip 이 없는 곳에서는 wheel 을 풀어서 씁니다. 한 번만 하면 되는 절차입니다.

1. 인터넷이 되는 곳에서 PyPI [files 페이지](https://pypi.org/project/hwpforge/#files)에서 대상 플랫폼의 wheel 을 받습니다 — Linux x86_64 면 `hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl`. sha256 도 그 페이지에 있습니다.
2. 대상 호스트의 조건을 확인합니다: CPython 3.9 이상, glibc 2.28 이상(`ldd --version`).
3. 파일을 옮겨 풉니다.

```console
python3 -c "import zipfile; zipfile.ZipFile('hwpforge-<version>-cp39-abi3-manylinux_2_28_x86_64.whl').extractall('/opt/hf')"
PYTHONPATH=/opt/hf python3 -c "import hwpforge; print(hwpforge.__version__)"
```

런타임 의존성이 없으므로 이것으로 끝입니다. 스크립트에서는 `sys.path.insert(0, "/opt/hf")` 로도 같은 효과를 냅니다.

## 문서 두 판을 비교해 검토 보고 만들기

```python
import hwpforge

base = hwpforge.Document.open("form.hwpx")
revised = base.fill({"user_email": "kim@example.com"}).document

d = base.diff(revised)
print("동일:", d["identical"])
print("바뀐 엔트리:", d["package"]["changed"])
for change in d["semantic"]["field_values"]:
    print("필드:", change)
for change in d["semantic"]["paragraphs"]:
    print("문단:", change)
```

`semantic` 의 다섯 목록(`field_values`·`cells`·`paragraphs`·`structure`·`raw`)은 각각 누름틀 값·셀 텍스트·문단 텍스트·구조·와이어 캐시의 차이입니다. 편집이 의도한 범위만 건드렸는지 보는 가장 빠른 검사는 `package["changed"]` 가 대상 구역 XML 하나뿐인지입니다.
