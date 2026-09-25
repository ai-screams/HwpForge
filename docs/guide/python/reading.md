# 문서 읽기와 검사

읽기 연산은 문서를 바꾸지 않고 보고서(`dict`)를 돌려줍니다. 문서는 열 때 디코드되지 않습니다 — `Document.open`은 바이트만 들고, 각 연산이 필요할 때 디코드합니다. 그래서 잘못된 바이트는 `open`이 아니라 첫 연산에서 `HwpForgeError(DECODE_FAILED)`로 드러납니다.

## 열기

```python
import hwpforge

doc = hwpforge.Document.open("hancom.hwpx")           # 파일에서, 100 MB 상한
same = hwpforge.Document.from_bytes(doc.to_bytes())    # 이미 든 바이트에서, 상한 없음

print(repr(doc), len(doc), doc == same)                # Document(<n> bytes) <n> True
```

`Document(data)` 생성자도 있지만 `open`/`from_bytes`가 바이트의 출처를 말해 주므로 그쪽을 쓰세요. `bytes`가 아닌 것을 넘기면 `TypeError`입니다.

## `inspect()` — 무엇이 얼마나 있는가

```python
report = doc.inspect()
print(report["sections"], report["paragraphs"], report["tables"], report["images"], report["charts"])
print(report["metadata"]["title"], report["metadata"]["author"], report["metadata"]["modified"])
print(report["fields"])                     # 누름틀 이름 목록
section = report["section_details"][0]
print(section["top_level_paragraphs"], section["all_tables"], section["has_header"])
```

`metadata`는 `title`·`author`·`subject`·`description`·`last_saved_by`·`created`·`modified`·`keywords`를 가지며, 날짜 둘은 없으면 `None`입니다.

`styles=True`를 주면 문서가 정의한 글꼴·글자 모양·문단 모양 요약이 `styles` 키로 더해집니다(기본은 생략).

```python
styles = doc.inspect(styles=True)["styles"]
print(styles["fonts"][0])          # {'id': 0, 'face_name': '함초롬돋움', 'lang': 'HANGUL'}
print(styles["char_shapes"][0])    # id, font_id, size_pt, bold, italic, color
print(styles["para_shapes"][0])    # id, alignment, line_spacing
```

### 섹션 계수 — 접두사가 곧 범위

`section_details[i]`의 키는 접두사로 **무엇을 셌는지**를 말합니다. 네 집합은 **서로 포함 관계가 아닙니다** — 접두사마다 재귀해 들어가는 곳이 달라, 어느 행도 다른 행을 넓힌 판본이 아닙니다.

| 접두사        | 재귀해 들어가는 곳                          | master page | 캡션   |
| ------------- | ------------------------------------------- | ----------- | ------ |
| `top_level_`  | 없음 — 섹션 본문 흐름 자체만                | 아니오      | 아니오 |
| (접두사 없음) | 셀·글상자·각주/미주·메모·머리말/꼬리말      | 예          | 아니오 |
| `deep_`       | 머리말/꼬리말·셀·글상자/주석/도형/메모      | 아니오      | 아니오 |
| `all_`        | 섹션 XML이 담은 전부(묶음 객체의 자식 포함) | 아니오      | 예     |

`top_level_`과 접두사 없는 키는 문단·표·이미지·차트를 세고, `deep_`은 문단만, `all_`은 여섯 가지 객체(`all_tables`·`all_images`·`all_text_boxes`·`all_lines`·`all_rectangles`·`all_polygons`)를 셉니다. `non_empty_` 중위사는 세는 집합을 바꾸지 않고 "보이는 텍스트가 있는 문단"으로만 좁히므로, `top_level_non_empty_paragraphs`는 `top_level_paragraphs`를 넘지 않습니다. `all_charts`는 없습니다 — 디코더가 섹션 최상위 문단 밖의 차트를 복원하지 못해, 그런 키를 두면 정작 필요한 문서에서 조용히 과소 계수하게 됩니다.

`all_` 키는 **디코드된 객체**를 세지, 원시 XML 요소를 세지 않습니다. CLI `--json`에는 같은 범위를 예전 철자(`tables`·`images`·`text_boxes` 등)로 내보내는 키가 있는데 그쪽은 원시 스캔이라, 디코더가 표현하지 못하는 요소를 받아들인 문서에서는 더 큰 값이 나올 수 있습니다. 두 값은 교환 가능하지 않습니다. 문단 키에는 이 단서가 붙지 않습니다.

## `outline()` — 제목·표·누름틀·책갈피의 위치

```python
outline = doc.outline()["outline"]
print(outline["title"])                                   # 없으면 None
for h in outline["headings"]:
    print(h["level"], h["text"], h["at"])                 # at = {'section': s, 'para': p}
for t in outline["tables"]:
    print(t["ordinal"], t["rows"], t["cols"], t["addressable"], t["caption"])
print(outline["fields"], outline["bookmarks"])
```

`tables[i]["ordinal"]`이 `read(table=…)`·`set_cell(table=…)`이 받는 표 번호입니다(0부터). `addressable`이 거짓인 표는 격자 주소로 셀을 지정할 수 없습니다(병합이 격자를 깨는 경우).

## `fields()` — 누름틀

```python
for f in hwpforge.Document.open("form.hwpx").fields()["fields"]:
    print(f["name"], f["hint"], f["current"], f["section"], f["fillable"])
# user_email 회사 이메일을 입력하세요 회사 이메일을 입력하세요 0 True
```

`fillable`이 거짓인 필드는 `fill`이 `FIELD_NOT_FILLABLE`로 거부합니다. `name`은 이름이 없는 필드에서 `None` 일 수 있습니다.

## `read()` — 한 부분만

문단 범위·표·누름틀 셋 중 **정확히 하나**를 지정합니다. 보고서의 세 키(`paragraphs`·`table`·`fields`)는 항상 있고, 묻지 않은 것은 `None`입니다.

```python
plain = hwpforge.Document.open("plain.hwpx")

view = plain.read(section=0, paras="0..2")["paragraphs"]     # 양끝 포함: 0, 1, 2
for p in view["paragraphs"]:
    print(p["at"]["para"], p["kind"], p["text"])

table = hwpforge.Document.open("grid.hwpx").read(table=0)["table"]
for cell in table["cells"]:
    print(cell["row"], cell["col"], cell["row_span"], cell["col_span"], repr(cell["text"]))

fields = hwpforge.Document.open("form.hwpx").read(field="user_email")["fields"]
print(fields[0]["current"])
```

- `paras`는 `"시작..끝"`이고 **양끝 포함**입니다. 범위가 섹션을 넘으면 `READ_PARA_RANGE_INVALID`, 형식이 틀리면 `READ_PARAS_INVALID`.
- 문단은 `kind`로 구분됩니다: `"body"`, `"heading"`(`level` 포함), `"list"`(`numbered`·`level`·`checked` 포함 — `checked`는 체크 목록이 아니면 `None`). 문단이 표·그림 등을 품으면 `contains` 키가 더해집니다.
- 표는 `rows`·`cols`와 셀 목록입니다. 병합된 영역은 기준 셀 하나로만 나오므로 셀 수가 `rows × cols`보다 적을 수 있습니다.
- 아무것도 안 주거나 둘 이상 주면 `READ_TARGET_REQUIRED`, 표 번호가 넘치면 `READ_TABLE_OUT_OF_RANGE`.

## `validate()` — 두 종류의 실패

```python
report = doc.validate()
print(report["ok"], report["sections"], report["paragraphs"])
for problem in report["errors"]:
    print(problem["code"], problem["message"])
```

문서가 **디코드는 되지만** 모델 불변조건을 어기면 예외 없이 `ok`가 거짓인 보고서를 돌려주고, 이유는 `errors`에 담깁니다. 반면 **디코드 자체가 안 되는** 입력은 연산 실패이므로 `HwpForgeError(DECODE_FAILED)`를 던집니다. 즉 "유효하지 않은 문서"는 반환값으로, "문서가 아닌 바이트"는 예외로 옵니다.

```python
try:
    hwpforge.Document.from_bytes(b"not a document").validate()
except hwpforge.HwpForgeError as exc:
    print(exc.code)        # DECODE_FAILED
```

CLI·MCP는 같은 필드를 `valid`라 부릅니다. Python은 `ok`입니다.

## `diff()` — 두 문서의 차이

```python
edited = plain.insert_para(section=0, anchor=1, text="새 문단").document
d = plain.diff(edited)
print(d["identical"])                      # False
print(d["package"]["changed"])             # ['Contents/section0.xml']
print(len(d["semantic"]["paragraphs"]), len(d["semantic"]["structure"]))
```

`semantic`은 디코드된 모델의 차이(`field_values`·`cells`·`paragraphs`·`structure`·`raw`), `package`는 ZIP 엔트리를 바이트로 비교한 결과(`added`·`removed`·`changed`)입니다. `note`가 이 두 층의 의미를 한 줄로 설명합니다. 조판 캐시처럼 디코드 모델에 담기지 않는 XML의 차이는 `raw`로 잡히며 `raw_dropped`가 생략된 개수를 말합니다.

## `stamp_plan()` — 스탬프할 자리 찾기

```python
plan = hwpforge.Document.open("template.hwpx").stamp_plan()
for c in plan["text"]:
    print(c["path"], c["span"], repr(c["marker"]), c["pattern"], c["guard"])
for c in plan["cells"]:
    print(c["table"], c["at"], [label["normalized"] for label in c["labels"]], c.get("suggested_name"))
print(plan["schema_version"], plan["source_sha256"][:12])
```

`text`는 `(   )`·`□` 같은 인라인 표시 후보, `cells`는 라벨 옆 빈 셀 후보입니다. `guard`가 있는 후보는 문맥상 채우면 안 될 가능성이 있는 자리(예: 안내문 안의 괄호)라 `stamp`가 승인 없이는 건드리지 않습니다. `source_sha256`은 이 계획이 어느 문서에서 나왔는지의 지문이고, 버전 있는 요청(`StampRequestV2`)에 그대로 들어갑니다. 계획을 실제 스탬프로 바꾸는 규칙은 [편집](editing.md#stamp--템플릿에-누름틀-심기)에 있습니다.

## `to_json()` · `export_section()` — 구조를 읽는 가장 넓은 창

`to_json()`은 문서 전체를, `export_section(section=i)`은 한 구역을 JSON으로 내보냅니다. 둘 다 `TextResult`라 `.text`(JSON 문자열)와 `.report`(같은 내용을 `dict`로 담은 `document`/`section` 키 + `warnings`)를 함께 줍니다. 두 JSON은 **서로 다른 스키마**입니다 — 전체 문서는 `from_json`이, 한 구역은 `patch`가 읽습니다. 자세한 것은 [변환과 내보내기](converting.md#to_json--from_json--전체-문서를-json으로)와 [편집](editing.md#patch--구역-json을-편집해-되돌려-넣기)에 있습니다.
