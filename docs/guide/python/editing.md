# 편집

모든 편집 메서드는 `DocumentResult`를 돌려줍니다 — `.document`가 새 문서, `.report`가 보고서입니다. 받은 문서는 바뀌지 않으므로 결과를 다시 받아야 합니다.

```python
import hwpforge

doc = hwpforge.Document.open("form.hwpx")
result = doc.fill({"user_email": "kim@example.com"})
doc = result.document          # 이 줄이 없으면 아무것도 바뀌지 않은 것입니다
```

편집기는 두 부류입니다. **보존 우선(preserve-first)** 편집기(`fill`·`patch`)는 건드리는 XML 조각만 바꾸고 나머지 바이트를 그대로 두며, **재인코드** 편집기(`set_cell`·`stamp`·`restyle`)는 문서를 모델로 디코드해 고친 뒤 패키지를 다시 씁니다. `insert_para`/`delete_para`는 구역 XML에서 해당 문단만 넣고 빼지만, 재인코드 편집기와 같은 사전 검사(원본 패키지의 엔트리가 재인코드 뒤에도 남는지, 디코드–인코드 왕복이 안전한지)를 통과해야 합니다. 이 차이가 아래 "한컴 저장 문서" 표를 만듭니다.

## HWP5 입력과 변환 손실

`Document.open("form.hwp")` 또는 `Document.from_bytes(data)`로 HWP5 입력을 받습니다. `inspect`, `outline`, `read`, `fields`, `validate`, `diff`(양쪽), `stamp_plan`, `to_md`, `to_json`, `export_section`과 편집 5종은 연산 전에 HWPX로 변환합니다. 확장자가 아니라 내용으로 판별하며, 편집 결과는 `.hwpx`입니다.

`fill`, `set_cell`, `insert_para`, `delete_para`, `stamp`는 의미 손실이 있으면 `HWP5_CONVERSION_LOSS`로 거부합니다. 거부 경고는 `error.details["warnings"]`에 있습니다. 경고를 검토한 뒤에만 키워드 `accept_conversion_loss=True`로 허용하세요. 변환 → 기존 연산의 검증·편집 → 변환 손실 거부 순서이며, 이 옵션은 편집 자체의 손실 거부를 해제하지 않습니다. 읽기는 변환 경고를 보고하며 손실 때문에 거부하지 않습니다.

```python
doc = hwpforge.Document.open("form.hwp")
print(doc.fields().get("conversion"))
result = doc.fill({"name": "홍길동"}, accept_conversion_loss=True)
result.document.save("filled.hwpx")
```

기존 `warnings`의 형과 내용은 그대로입니다. 변환된 입력에는 별도 `conversion = {"source_format": "hwp5", "warnings": [...]}`가 더해집니다. 각 경고는 `stage`, `code`, `message`와 선택적인 `location`을 갖습니다. `diff`는 `conversion.base`·`conversion.revised`에 변환된 쪽만 싣습니다. JSON·Markdown 문서 본문에는 이 메타데이터를 넣지 않습니다.

`save`는 `None`을 반환합니다. HWP5를 그 자리에서 변환해 저장하면 파일 기록이 성공한 뒤 수락한 손실과 비손실 변환 경고를 코드와 알려진 위치를 담은 표준 `UserWarning`으로 알립니다. Python 경고 필터가 그대로 적용되어 기본 필터는 같은 경고를 한 번만 보여 주고, `error` 필터는 파일을 쓴 뒤 예외를 냅니다. 전체 목록은 읽기·편집 보고서의 `conversion`이나 `warnings.simplefilter("always")`를 넣은 `warnings.catch_warnings(record=True)`로 받습니다.

`save`는 `.hwp` 확장자(대소문자 무시)를 `INVALID_EXTENSION`으로 거부합니다. 원본 HWP5를 그대로 저장하려면 HWPX로 변환하며, 의미 손실은 기본 거부합니다 (`save(..., accept_conversion_loss=True)`로 명시적 허용). `open`이 읽은 파일의 정체성은 편집 결과에도 이어집니다. 원본 경로가 삭제되거나 교체되어도 당시 읽은 HWP5 파일의 심볼릭 링크·하드 링크·상대 경로 별칭에 저장하면 `INVALID_OUTPUT_PATH`로 거부합니다. 보호 대상은 `open` 때 읽은 파일이며 현재 그 경로가 가리키는 파일이 아닙니다. 파일은 장치와 inode로 식별하므로, 삭제된 원본의 inode를 파일 시스템이 다시 배정한 새 파일도 거부합니다. 이때는 다른 경로에 저장합니다. 저장 시 출력의 정체성만 조회하며, 출력 부재는 허용하고 그 밖의 조회 실패는 `FILE_WRITE_FAILED`입니다. HWPX의 제자리 저장은 허용합니다. 저장 호출 중 다른 프로세스가 출력 경로를 바꾸는 상황은 보호하지 않습니다.

`Document(data)`·`from_bytes`, `convert_hwp5`·`convert_md`·`from_json`의 생성 결과에는 원본 파일 정체성이 없습니다. 이 경우 원본 별칭인지 판별할 수 없으므로 호출자가 출력 경로를 구분해야 합니다. bytes·텍스트를 반환하는 연산은 파일을 쓰지 않습니다. `from_json(base=...)`, `patch`, `restyle`의 HWP5 자동 변환은 이번 범위 밖이며 계속 `HWP5_INPUT`을 반환합니다. 명시적으로 변환한 HWPX를 넘기세요.

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

- 이름이 없는 필드는 `FIELD_NOT_FOUND`(메시지에 사용 가능한 이름 목록이 붙습니다), 채울 수 없는 필드는 `FIELD_NOT_FILLABLE`, 빈 문자열은 `EMPTY_FIELD_VALUE`, 값 맵이 비어 있으면 `NO_VALUES`입니다. 값은 문자열이어야 하며 다른 형은 `TypeError`입니다.
- 같은 이름의 필드가 문서에 둘 이상이면 어느 쪽인지 모호하므로 자동으로 전부 채우지 않고 `FIELD_NAME_AMBIGUOUS`로 거부합니다(아무것도 쓰지 않습니다). 문서에서 이름을 유일하게 만든 뒤 다시 채우세요.
- `fill`과 `patch`는 본문 문단이 편집 전후 한 줄 안에 들어간다고 확인되면 그 문단의 원래 줄 조판 캐시를 남깁니다. 한글·숫자·일부 문장부호와 공백으로 된 글에 한하며, 문단 전체의 글자 모양과 문자 종류별 크기·간격 설정이 같고 글자 효과(굵게·기울임·그림자 등), 커닝, 강조점, 글꼴 공백 설정이 없어야 합니다. 편집 가능한 누름틀과 앞뒤 글은 합쳐서 판단합니다. 편집 가능한 누름틀이 아닌 필드(책갈피와 편집할 수 없는 누름틀 포함)가 든 문단은 자기 캐시를 잃습니다. 그런 필드의 위치를 확인할 수 없으면 그 구역의 모든 편집 문단이 자기 캐시를 잃습니다. 여러 글 조각으로 나뉘거나 여러 문단에 걸치는 누름틀도 편집할 수 없는 누름틀에 해당합니다. 화면에 보이는 글 전체를 확인할 수 없거나, 폭을 넘거나, 탭·강제 줄바꿈·개체·빈 누름틀이 있거나, 원본 설정을 확인할 수 없으면 캐시를 지워 한컴이 줄을 다시 나누게 합니다. 표 칸·각주·머리말 등 내부 문단은 이 자기 캐시 보존 대상이 아닙니다. 아래의 중첩 편집·구역·뒤 문단 제거 규칙이 적용되면 한 줄인 본문도 캐시를 지웁니다. 본문 문단을 고쳤을 때 그 뒤의 문단 가운데 글자와 누름틀만 담은 문단은 캐시를 그대로 둡니다. 그런 문단은 앞 문단이 길어져도 줄 나눔이 그대로이고, 한컴이 줄의 세로 위치만 다시 쌓기 때문입니다. 다음 경우에는 뒤 문단의 캐시도 함께 지웁니다.
  - 표·그림·도형 같은 개체, 누름틀이 아닌 필드(상호 참조, 날짜, 계산식, 하이퍼링크 등)의 어느 부분이나 쪽 번호를 담은 뒤 문단(필드는 여러 문단에 걸칠 수 있고, 고친 문단에서 시작한 필드도 해당합니다): 그런 첫 문단부터 그 흐름 끝까지 지웁니다. 쪽이 바뀌면 이런 문단의 폭이나 표시 글자가 달라질 수 있습니다.
  - 구역 어딘가(표 칸, 머리말·꼬리말 포함)에 글자처럼 취급하지 않는 개체가 있고 본문이 그 둘레를 감싸는 경우(배치 "어울림" 등): 고친 문단이 속한 흐름(그 흐름을 품은 바깥 흐름 포함)의 뒤 문단을 모두 지웁니다. 개체는 제자리에 있는데 문단만 밀리면 감싼 줄의 모양이 달라지기 때문입니다. 뒤에 오는 표 칸·글상자 안의 문단은 캐시를 그대로 둡니다.
  - 세로쓰기 구역이나 단 폭이 서로 다른 다단 구역: 고친 문단이 속한 흐름(그 흐름을 품은 바깥 흐름 포함)의 뒤 문단을 모두 지웁니다. 뒤에 오는 표 칸·글상자 안의 문단은 캐시를 그대로 둡니다.
  - 구역 어딘가(머리말·꼬리말 포함)에 문서 속 자리를 확인할 수 없는 필드가 있는 경우: 시작과 끝이 맞지 않는 필드(끝이 없는 필드 등)나 위치를 알 수 없는 필드(따로 저장된 바탕쪽에 있는 필드 등)가 해당합니다. 다만 위치를 찾지 못한 필드가 책갈피나 편집할 수 없는 누름틀이면, 그 구역의 편집한 본문 문단은 자기 캐시만 잃고 뒤의 평문 문단은 캐시를 유지합니다. 그 밖의 필드 위치를 찾지 못하거나 필드의 시작·끝 불일치 등 오류·경고가 있으면, 전과 같이 고친 문단이 속한 흐름(그 흐름을 품은 바깥 흐름 포함)의 뒤 문단까지 지웁니다. 구역의 다른 문단은 캐시를 그대로 둡니다.
  - 표 칸·글상자·각주·미주 안의 문단을 고친 경우: 그 안의 뒤 문단과, 그 표·글상자·주석이 놓인 문단 및 그 뒤 문단을 지웁니다.
  - 머리말·꼬리말 안의 문단을 고친 경우: 그 머리말·꼬리말 안의 뒤 문단만 지웁니다. 본문 캐시는 그대로 둡니다.
  - 스타일 정의를 고쳐 써야 하는 문서라서 `patch`가 문서를 다시 쓰는 경우: 위 규칙 대신 재생성(`from_json(base=...)`)의 규칙을 따르므로 더 많은 캐시가 지워질 수 있습니다. 쪽 번호나 떠 있는 개체가 있으면 구역이나 문서 전체의 캐시가 지워지기도 합니다([재생성의 캐시 규칙](../format-pipeline.md#cli에서-포맷-처리)).

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

한 문단·한 줄인 칸은 짧은 값으로 바꾼 뒤에도 줄 조판 캐시를 남길 수 있습니다. 글자 효과(굵게·기울임·그림자 등), 커닝, 강조점, 글꼴 공백 설정이 있는 칸은 보존 대상에서 제외합니다. 문자 종류별 장평·자간·상대 크기·글자 위치 설정이 모두 기록되어 있고 서로 같아야 하며, 원본 설정과 새 글의 폭을 확인할 수 없는 경우에도 캐시를 지웁니다.

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
