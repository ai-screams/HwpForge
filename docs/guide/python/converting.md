# 변환과 내보내기

## `convert_md` — Markdown 에서 문서 만들기

```python
import hwpforge

source = """---
title: 보고서
---

# 개요

본문입니다.

| 항목 | 값 |
| -- | -- |
| 예산 | 1,000 |
"""
result = hwpforge.convert_md(source, preset="classic")
print(result.report["sections"], result.report["paragraphs"])
print(result.document.inspect()["metadata"]["title"])     # 보고서
result.document.save("report.hwpx")
```

- Markdown 은 GFM 이고, 맨 앞의 YAML frontmatter 가 문서 메타데이터와 스타일을 정합니다(`title` 등). 스타일 문법은 [Markdown 에서 HWPX 로](../markdown-bridge.md)를 보세요.
- `preset` 은 `hwpforge.templates()` 가 나열하는 이름 중 하나입니다(`default`·`modern`·`classic`·`latest`). 없는 이름은 `PRESET_NOT_FOUND`.
- 이미지 `![…](path)` 는 `base_dir` 를 기준으로 읽습니다. `base_dir` 를 주지 않으면 파일 이미지는 **버려지고** 보고서에 남습니다. `base_dir` 밖의 경로는 읽지 않습니다.

```python
result = hwpforge.convert_md("# 그림\n\n![도표](chart.png)\n")     # base_dir 없음
print(result.report["assets"])
# [{'kind': 'dropped', 'occurrence': {'paragraph': 1, 'run': 0}, 'reason': 'no_base_dir'}]
print([w["code"] for w in result.report["warnings"]])              # ['IMAGE_EMBED_SKIPPED']
```

`assets` 의 각 항목은 `kind` 로 구분됩니다: `embedded`(`key`·`format` 포함), `dropped`(`reason` 포함), `remote`(URL 은 내려받지 않습니다).

## `to_md` — 문서를 Markdown 으로

```python
doc = hwpforge.Document.open("hancom.hwpx")
styled = doc.to_md()                       # mode="styled": 스타일 frontmatter 유지
lossy = doc.to_md(mode="lossy")            # Markdown 이 못 담는 것은 버리고 warnings 로 알림
print(lossy.text[:200])
print(lossy.report["mode"], list(lossy.report["images"].keys()))
```

- `mode="lossless"` 는 무언가를 잃어야 하면 `ENCODE_FAILED` 로 거부합니다.
- 보고서의 `images` 는 문서가 참조한 이미지의 `{키: 바이트}` 입니다. Markdown 텍스트가 그 키를 가리키므로, 파일로 저장할 때 같은 이름으로 옆에 써 두면 됩니다.

## `to_json` · `from_json` — 전체 문서를 JSON 으로

```python
import json

exported = doc.to_json()                            # styles=True 가 기본
document = json.loads(exported.text)                # 키: document, styles
print(exported.report["document"] == document)      # 같은 내용을 dict 로도 줍니다

rebuilt = hwpforge.from_json(exported.text, base=doc)
print(rebuilt.report["paragraphs"], [w["code"] for w in rebuilt.report["warnings"]])
# 3 ['LAYOUT_CACHE_DROPPED']
```

- `to_json` 의 JSON 은 문서 전체와 style store 이며, 표 셀에 격자 주소가 붙어 있습니다. `styles=False` 는 구조만 내보냅니다.
- `from_json` 은 그 JSON 을 **다시 인코드**해 새 문서를 만듭니다. `base` 를 주면 style store(스타일 없이 내보낸 JSON 일 때)와 이미지 바이너리를 그 문서에서 가져옵니다. `base` 없이 이미지가 있는 문서를 재구성하면 이미지가 빠집니다.
- 생성은 fail-closed 가 아닙니다: 의미가 손실돼도 거부하지 않고 `warnings` 에 실어 돌려줍니다. 위의 `LAYOUT_CACHE_DROPPED` 는 원본이 가진 줄 조판 캐시를 재인코드가 싣지 않는다는 뜻입니다(한컴에서 다시 저장하면 복원).
- 한컴 저장 문서의 `Preview/*`·`container.rdf` 엔트리는 `base` 를 줘도 승계되지 않습니다(경고 없음). 텍스트만 바꿀 거라면 `export_section` → `patch` 가 문서를 온전히 보존합니다.
- JSON 이 문서가 아니면 `JSON_PARSE_FAILED`. 스키마는 `hwpforge.schema(kind="exported-document")`.

전체 JSON(`document`/`styles`)과 구역 JSON(`section_index`/`section`/`styles`/`preservation`)은 서로 다른 스키마입니다 — 전자는 `from_json`, 후자는 `patch` 가 읽습니다.

## `convert_hwp5` — 옛 `.hwp` 를 HWPX 로

```python
with open("old.hwp", "rb") as handle:
    converted = hwpforge.convert_hwp5(handle.read())
print(len(converted.document), [w["code"] for w in converted.report["warnings"]])
converted.document.save("old.hwpx")
```

- 인자는 파일 경로가 아니라 **바이트**입니다(`Document.open` 은 HWPX 전용입니다). 읽을 수 없는 바이트는 `HWP5_DECODE_FAILED`.
- 보고서의 `warnings` 에 HWP5 가 가졌지만 HWPX 로 옮기지 못한 것이 전부 나옵니다.
- `carry_layout_cache=True` 는 한컴이 계산해 둔 줄 조판 캐시를 함께 옮깁니다. PDF 렌더가 한컴의 쪽 나눔을 그대로 재현하려면 이 캐시가 필요합니다(아래). 캐시를 옮긴 HWPX 는 PDF 재생·비교용이며, 한컴에서 다시 열어 편집할 문서로 취급하지 마세요.

## `to_pdf` — PDF 렌더

렌더는 문서에 저장된 조판을 **재생**합니다(다시 계산하지 않습니다). 그래서 조판 캐시를 가진 문서만 렌더됩니다: 한컴이 저장한 HWPX, 그리고 `convert_hwp5(..., carry_layout_cache=True)` 로 캐시를 옮긴 HWPX. Markdown·JSON 에서 생성한 문서는 캐시가 없어 `PDF_RENDER_FAILED` 입니다.

```python
with open("old.hwp", "rb") as handle:
    carried = hwpforge.convert_hwp5(handle.read(), carry_layout_cache=True).document

try:
    pdf = carried.to_pdf(font_dirs=["/Library/Fonts/Hancom"])
    print(pdf.report["pages"])
    with open("old.pdf", "wb") as out:
        out.write(pdf.data)
except hwpforge.HwpForgeError as exc:
    print(exc.code, exc.cause)
    # PDF_RENDER_FAILED {'stage': 'render', 'code': 'FONT_UNRESOLVED'}      ← 글꼴을 못 찾음
    # PDF_RENDER_FAILED {'stage': 'render', 'code': 'NO_RENDERABLE_CACHE', 'location': 's0'}  ← 캐시 없음
```

- 글꼴은 기본 fail-closed 입니다: 문서가 이름 붙인 글꼴 face 를 `font_dirs` 안에서 찾지 못하면 추측하지 않고 실패합니다(`cause["code"] == "FONT_UNRESOLVED"`). `degraded=True` 는 대체 글꼴로 렌더하며, 결과의 모양이 달라집니다.
- `discovery` 는 `font_dirs` 밖을 더 볼지의 선택입니다: `"explicit"`(기본, 결정적) · `"hancom"`(한컴 설치 글꼴 위치) · `"platform"`(OS 글꼴).
- `font_dirs` 는 시퀀스여야 합니다. 문자열 하나를 주면 한 글자짜리 디렉터리들로 읽히는 대신 `TypeError` 로 거부됩니다.
- `partial_cache_reject=True` 는 캐시가 일부만 있는 문서를 다시 배치하지 않고 거부합니다.
- 실패의 두 번째 분류는 `exc.cause` 에 옵니다(`stage`·`code`·`kind`·`location`). `exc.hint` 가 어느 쪽인지 안내합니다.

## `templates` · `schema`

```python
for preset in hwpforge.templates()["presets"]:
    print(preset["name"], preset["description"], preset["font"], preset["page_size"])

schema = hwpforge.schema(kind="exported-section")   # "document" | "exported-document" | "exported-section"
print(schema["title"], list(schema["properties"])[:4])
```

두 함수의 결과에는 `warnings` 가 없습니다. `schema` 는 JSON Schema `dict` 이며 키는 스키마 자신의 것입니다.
