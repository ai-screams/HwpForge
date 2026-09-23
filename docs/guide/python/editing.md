# 편집

모든 편집 메서드는 `DocumentResult`를 돌려줍니다 — `.document`가 새 문서, `.report`가 보고서입니다. 받은 문서는 바뀌지 않으므로 결과를 다시 받아야 합니다.

```python
import hwpforge

doc = hwpforge.Document.open("form.hwpx")
result = doc.fill({"user_email": "kim@example.com"})
doc = result.document          # 이 줄이 없으면 아무것도 바뀌지 않은 것입니다
```

편집기는 두 부류입니다. **보존 우선(preserve-first)** 편집기(`fill`·`patch`)는 건드리는 XML 조각만 바꾸고 나머지 바이트를 그대로 두며, **재인코드** 편집기(`set_cell`·`stamp`·`restyle`)는 문서를 모델로 디코드해 고친 뒤 패키지를 다시 씁니다. `insert_para`/`delete_para`는 구역 XML에서 해당 문단만 넣고 빼지만, 재인코드 편집기와 같은 사전 검사(원본 패키지의 엔트리가 재인코드 뒤에도 남는지, 디코드–인코드 왕복이 안전한지)를 통과해야 합니다. 이 차이가 아래 "한컴 저장 문서" 표를 만듭니다.

## 한컴 저장 문서에서 되는 것과 안 되는 것

한컴 오피스가 저장한 `.hwpx`에는 HwpForge 인코더가 재현하지 않는 패키지 엔트리(`Preview/PrvText.txt`·`Preview/PrvImage.png`·`META-INF/container.rdf`)가 들어 있습니다. 네 연산은 이런 문서를 조용히 손상시키는 대신 **fail-closed로 거부**합니다.

| 연산                                           | 한컴 저장 문서 | 결과                                                              |
| ---------------------------------------------- | -------------- | ----------------------------------------------------------------- |
| `fill`                                         | 동작           | 누름틀만 쓰고 나머지 엔트리는 바이트 그대로 보존                  |
| `patch`                                        | 동작           | 기존 문단·셀의 텍스트만 바꾸고 패키지를 보존                      |
| `set_cell`·`insert_para`·`delete_para`·`stamp` | 거부           | `INPUT_ENTRIES_NOT_CARRIED` 또는 `INPUT_NOT_ROUNDTRIP_SAFE`       |
| `restyle`                                      | 동작           | 재인코드하므로 위 엔트리와 조판 캐시가 사라지며 `warnings`로 알림 |
| `from_json(base=...)`                          | 부분 승계      | 이미지만 승계하고 위 엔트리는 버려집니다(별도 경고 없음)          |

```python
hancom = hwpforge.Document.open("hancom.hwpx")
try:
    hancom.set_cell(table=0, at="0,0", text="값")
except hwpforge.HwpForgeError as exc:
    print(exc.code)     # INPUT_ENTRIES_NOT_CARRIED
    print(exc.hint)     # 되는 길: 텍스트는 export_section → 편집 → patch, 누름틀은 fill
```

따라서 실제 정부 서식을 다룰 때 통하는 길은 둘입니다: 누름틀은 `fill`, 그 밖의 텍스트는 `export_section` → 편집 → `patch`. 구조를 정말 바꿔야 할 때만 `from_json(base=...)`을 쓰고, 결과를 한컴에서 열어 확인하세요. 이 제한을 푸는 작업이 진행 중이며(GitHub #143), 풀리면 이 표가 바뀝니다.

## `fill` — 누름틀 채우기

```python
form = hwpforge.Document.open("form.hwpx")
result = form.fill({"user_email": "kim@example.com"})
print(result.report["filled"])
# [{'name': 'user_email', 'section': 0, 'previous': '회사 이메일을 입력하세요'}]
print(result.document.read(field="user_email")["fields"][0]["current"])
# kim@example.com
```

- 이름이 없는 필드는 `FIELD_NOT_FOUND`(메시지에 사용 가능한 이름 목록이 붙습니다), 채울 수 없는 필드는 `FIELD_NOT_FILLABLE`, 빈 문자열은 `EMPTY_FIELD_VALUE`입니다. 값은 문자열이어야 하며 다른 형은 `TypeError`입니다.
- 같은 이름의 필드가 여럿이면 전부 채워지고 `filled`에 각각 한 줄씩 나옵니다.
- `fill`은 바꾼 문단의 조판 캐시만 무효화하고 나머지는 보존하므로, 한컴 저장 문서에서도 안전합니다.

```python
try:
    form.fill({"nope": "x"})
except hwpforge.HwpForgeError as exc:
    print(exc.code, "|", exc.message)
    # FIELD_NOT_FOUND | field 'nope' not found; available: [user_email]
```

## `patch` — 구역 JSON을 편집해 되돌려 넣기

텍스트를 바꾸는 보존 우선 경로입니다. `export_section`이 낸 JSON의 텍스트를 고쳐 `patch`로 넣으면 그 구역 XML의 텍스트 슬롯만 치환됩니다.

```python
import json

exported = hancom.export_section(section=0)
section = json.loads(exported.text)          # 키: section_index, section, styles, preservation
patched = hancom.patch(section=0, patch=json.dumps(section, ensure_ascii=False))
print(patched.report)                        # {'section': 0, 'warnings': []}
print(hancom.diff(patched.document)["identical"])   # 바꾼 게 없으면 True
```

- JSON의 **텍스트만** 바꿀 수 있습니다. 문단을 더하거나 빼거나 표 구조·서식 참조를 바꾸면 `PATCH_FAILED`(구조 변경 감지)로 거부됩니다. 구조 변경은 `insert_para`/`delete_para`/`set_cell` 또는 `from_json`의 몫입니다.
- `preservation` 키는 텍스트 슬롯과 원본 바이트 스팬의 대응표입니다. 손대지 말고 그대로 돌려보내세요.
- JSON이 구역 스키마에 맞지 않으면 `JSON_PARSE_FAILED`, 격자 주소가 더 이상 맞지 않으면 `PATCH_FAILED`입니다. 스키마는 `hwpforge.schema(kind="exported-section")`으로 받을 수 있습니다.

텍스트를 실제로 바꾸는 예:

```python
section = json.loads(hancom.export_section(section=0).text)
first_run = section["section"]["paragraphs"][0]["runs"][0]
if "text" in first_run:
    first_run["text"] = "바뀐 첫 문단"
doc2 = hancom.patch(section=0, patch=json.dumps(section, ensure_ascii=False)).document
```

문단의 `runs[i]`는 텍스트 런이면 `text` 키를, 표·그림 같은 컨트롤이면 다른 키를 가집니다. 표 셀의 텍스트도 같은 JSON 안에 있으므로(`runs[i]["table"]["rows"][r]["cells"][c]["paragraphs"]…`) 한컴 저장 문서의 셀 텍스트는 이 길로 바꿉니다.

## `set_cell` — 표 셀에 쓰기

격자 주소(`at`)나 라벨 기준(`right_of`·`below`)으로 셀 하나를 지정하거나, `specs`로 여럿을 한 번에 씁니다. 두 형태는 배타적입니다.

```python
grid = hwpforge.Document.open("grid.hwpx")
print([(c["row"], c["col"], c["text"]) for c in grid.read(table=0)["table"]["cells"]])
# [(0, 0, '성명'), (0, 1, ''), (1, 0, '비고'), (1, 1, ''), (2, 1, '')]

one = grid.set_cell(table=0, at="0,1", text="홍길동")
print(one.report["results"][0])
# {'table': 0, 'requested': {'row': 0, 'col': 1}, 'anchor': {'row': 0, 'col': 1}, 'resolution': 'exact', 'cleared': False}

by_label = grid.set_cell(table=0, right_of="성명", text="홍길동")

many = grid.set_cell(specs=[
    {"table": 0, "at": {"row": 0, "col": 1}, "text": "홍길동"},
    {"table": 0, "right_of": "비고", "text": "없음"},
])
print([r["resolution"] for r in many.report["results"]])
```

- `at`은 메서드 인자로는 `"row,col"` 문자열, `specs` 안에서는 `{"row": r, "col": c}` 객체입니다(0부터).
- `right_of`/`below`는 그 텍스트를 가진 셀의 오른쪽/아래 셀입니다. 라벨이 없거나 그 방향에 셀이 없으면 `CELL_NOT_FOUND`, 라벨이 여럿이면 모호함으로 거부됩니다.
- 병합된 영역을 지정하면 기준 셀로 해석되며 `resolution`이 그 사실을 말합니다(`exact`가 아닌 값). 표가 격자 주소를 지원하지 않으면(`outline`의 `addressable` 거짓) 거부됩니다.
- 표 번호는 `outline()["outline"]["tables"][i]["ordinal"]`이며, 없으면 `TABLE_NOT_FOUND`.
- 재인코드 편집기이므로 한컴 저장 문서는 `INPUT_ENTRIES_NOT_CARRIED`/`INPUT_NOT_ROUNDTRIP_SAFE`로 거부됩니다. 그 문서의 셀 텍스트는 `patch`로 바꾸세요.

## `insert_para` / `delete_para` — 문단 넣고 빼기

```python
plain = hwpforge.Document.open("plain.hwpx")
print([p["text"] for p in plain.read(section=0, paras="0..3")["paragraphs"]["paragraphs"]])
# ['첫째 문단입니다.', '둘째 문단입니다.', '셋째 문단입니다.', '넷째 문단입니다.']

ins = plain.insert_para(section=0, anchor=1, text=["새 문단 하나", "새 문단 둘"])
print(ins.report)                    # {'inserted': 2, 'deleted': 0, 'warnings': []}

above = plain.insert_para(section=0, anchor=1, text="위에", before=True)

dl = ins.document.delete_para(section=0, indexes=[2, 3])
print(dl.report)                     # {'inserted': 0, 'deleted': 2, 'warnings': []}
```

- `text`는 문자열 하나 = 문단 하나, 시퀀스 = 원소마다 문단 하나입니다. 문자열을 글자 단위 시퀀스로 풀지 않습니다. 시퀀스의 원소는 `str`만 받습니다.
- `anchor`/`indexes`는 그 구역의 **최상위** 문단 인덱스입니다(표 셀 안의 문단은 세지 않습니다). 범위를 넘으면 `PARAGRAPH_OUT_OF_RANGE`.
- 삭제는 전부-아니면-무(all-or-nothing)이며 fail-closed 정책이 있습니다: 구역 속성을 가진 첫 문단(`SECTION_PROPERTIES_PARAGRAPH`), 책갈피·상호참조·각주 등 참조를 가진 문단, 쪽/단 나눔을 가진 문단(`HARD_BREAK_LOSS`), 구역을 비우는 삭제는 거부됩니다. 같은 이유로 첫 문단 앞에는 넣을 수 없습니다(`INSERT_BEFORE_SECTION_PROPERTIES`).
- 두 연산은 대상 구역 XML만 바꾸고 다른 엔트리는 바이트 그대로 둡니다. 다만 현재는 재인코드 편집기와 같은 사전 검사를 거치므로 한컴 저장 문서를 `INPUT_ENTRIES_NOT_CARRIED`로 거부합니다(위 표).

```python
try:
    plain.delete_para(section=0, indexes=[0])
except hwpforge.HwpForgeError as exc:
    print(exc.code)      # SECTION_PROPERTIES_PARAGRAPH
```

## `stamp` — 템플릿에 누름틀 심기

`stamp_plan`이 찾은 후보를 누름틀로 바꾸는 연산입니다. 흐름은 계획 → 명세(spec) 작성 → 스탬프 이고, **가드 없는 후보는 전부 이름을 붙이거나 `"ignore"`로 명시**해야 합니다 — 하나라도 빠지면 `STAMP_CANDIDATE_UNCOVERED`로 아무것도 쓰지 않습니다.

```python
template = hwpforge.Document.open("template.hwpx")
plan = template.stamp_plan()

specs = []
for i, candidate in enumerate(plan["text"]):
    spec = dict(candidate)                     # section, path, span, marker 를 그대로 복사
    if i == 0:
        spec["action"] = {"field": {"name": "applicant", "hint": "신청인 이름"}}
    else:
        spec["action"] = "ignore"
    specs.append(spec)

stamped = template.stamp(specs)
print(stamped.report["stamped"])
# [{'name': 'applicant', 'section': 0, 'path': 'paragraphs[0].runs[0].text', 'span': {...}, 'marker': '(   )', 'pattern': 'paren_blank'}]
print(stamped.report["ignored"], stamped.report["skipped_guarded"])
print([(f["name"], f["hint"]) for f in stamped.document.fields()["fields"]])
# [('applicant', '신청인 이름')]
```

명세 작성 규칙:

- **텍스트 후보**: 계획의 후보 객체를 복사하고 `action`만 더합니다. `section`·`path`·`span`·`marker`가 빠지면 거부되고, 여분 키(`pattern`·`guard`)는 무시됩니다.
- **셀 후보**: 복사하지 말고 `table`·`at`·`action`으로 새로 만듭니다(`label`을 붙이면 `text`는 계획의 `labels[].normalized`여야 합니다). 후보 객체를 통째로 넘기면 알 수 없는 키로 거부됩니다.
- `action`은 `"ignore"` 또는 `{"field": {"name": ..., "hint": ...}}`입니다.
- 버전 있는 요청은 `{"schema_version": plan["schema_version"], "source_sha256": plan["source_sha256"], "text": [...], "cells": [...]}`이며, 지문이 문서와 다르면 거부됩니다 — 계획을 만든 문서에만 적용된다는 뜻입니다.

```python
request = {
    "schema_version": plan["schema_version"],
    "source_sha256": plan["source_sha256"],
    "text": specs,
}
stamped = template.stamp(request, manifest=False)      # 보고서에서 manifest 생략
```

보고서의 `manifest`는 무엇을 어디에 심었는지의 기록(`schema_version`·`source_sha256`·`output_sha256`·`fields`)이고, `stamped`/`stamped_cells`는 적용 단계의 결과입니다. 재인코드 편집기이므로 의미가 손상될 인코딩은 `ENCODE_SEMANTIC_LOSS`로 거부하며(`exc.details`에 원인 경고), 한컴 저장 문서는 편집 전 사전 검사에서 거부됩니다.

## `restyle` — 다른 프리셋으로

```python
print([p["name"] for p in hwpforge.templates()["presets"]])   # ['default', 'modern', 'classic', 'latest']
restyled = hancom.restyle(preset="modern")
print(restyled.report["preset"], restyled.report["paragraphs"])
print([w["code"] for w in restyled.report["warnings"]])
```

문서를 다시 인코드하므로 조판 캐시와 인코더가 모르는 엔트리는 사라지고 그 사실이 `warnings`로 옵니다. 없는 프리셋은 `PRESET_NOT_FOUND`, 의미 손상은 `ENCODE_SEMANTIC_LOSS`입니다.

## 편집 뒤에 할 일

1. `result.report["warnings"]`를 읽습니다. 비어 있어야 정상이고, `LAYOUT_CACHE_DROPPED` 같은 코드는 한컴에서 다시 저장해야 쪽 배치가 복원된다는 뜻입니다.
2. `result.document.validate()["ok"]`로 모델이 유효한지 봅니다.
3. `original.diff(result.document)`로 바뀐 범위가 의도와 같은지 봅니다 — `package["changed"]`가 대상 구역 하나뿐인지가 좋은 검사입니다.
4. `save` 합니다. 파일명은 `.hwpx`로.
