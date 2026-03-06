# HWPX 포맷 주의사항 (Gotchas)

HWPX 포맷은 KS X 6101 스펙과 한글 실제 동작 사이에 차이가 있습니다. 이 페이지는 HwpForge를 사용하거나 HWPX 파일을 직접 생성할 때 반드시 알아야 할 30가지 함정을 정리한 참고 문서입니다. 각 항목은 실제 개발 과정에서 발견된 버그와 충돌 사례를 기반으로 합니다.

---

## 색상/단위 (Color & Unit)

````admonish danger title="1. 색상은 BGR 순서 (RGB 아님)"
HWP 포맷은 BGR(Blue-Green-Red) 바이트 순서를 사용합니다. 원시 16진수 값을 그대로 쓰면 색이 반전됩니다.

```rust
// ❌ WRONG — 0xFF0000은 HWP에서 파란색!
let red_bgr = 0xFF0000;

// ✅ CORRECT — from_rgb() 생성자를 항상 사용
Color::from_rgb(255, 0, 0)  // 내부적으로 0x0000FF로 저장
```
````

````admonish info title="2. HwpUnit은 정수 기반 단위"
부동소수점 정밀도 오류를 피하기 위해 HwpUnit은 정수 기반입니다. 1pt = 100 HWPUNIT, 1mm ≈ 283 HWPUNIT.

```rust
HwpUnit::from_pt(12.0)  // 12pt → HwpUnit(1200)
HwpUnit::from_mm(10.0)  // 10mm → HwpUnit(2834)
// 유효 범위: ±100M
```
````

---

## 네임스페이스 (Namespace)

````admonish danger title="3. 선(Line) 도형의 좌표는 hc: 네임스페이스"
기하 좌표에는 `hp:` (paragraph) 네임스페이스가 아닌 `hc:` (core) 네임스페이스를 사용해야 합니다. `hp:`를 쓰면 한글이 파일을 파싱하지 못합니다.

```xml
<!-- ❌ WRONG — 한글 parse error -->
<hp:startPt x="0" y="0"/>

<!-- ✅ CORRECT -->
<hc:startPt x="0" y="0"/>
```
````

````admonish danger title="4. 다각형 꼭짓점도 hc: 네임스페이스"
다각형(polygon)의 꼭짓점도 동일한 규칙이 적용됩니다. `hp:pt`를 쓰면 한글에서 "파일을 읽거나 저장하는데 오류"가 발생합니다.

```xml
<!-- ❌ WRONG — 한글 파일 오류 -->
<hp:pt x="0" y="0"/>

<!-- ✅ CORRECT (KS X 6101: type="hc:PointType") -->
<hc:pt x="0" y="0"/>
```

모든 기하 요소(선, 타원, 다각형, 글상자 모서리)는 `hc:` 네임스페이스를 사용합니다.
````

---

## 도형/차트/수식 (Shapes, Charts, Equations)

````admonish warning title="5. TextBox는 control 요소가 아님"
HWPX에서 글상자(TextBox)는 `Control` 요소가 아닙니다. `<hp:rect>` 도형 안에 `<hp:drawText>`를 내포한 구조입니다.

```rust
// ❌ WRONG (HWPX는 control 요소가 아님)
Control::TextBox(...)

// ✅ CORRECT (HWPX 실제 구조)
// <hp:rect ...><hp:drawText>...</hp:drawText></hp:rect>
```
````

````admonish warning title="6. 수식(Equation)에는 shape common 블록이 없음"
수식은 일반 도형(선/타원/다각형)과 달리 offset, orgSz, curSz, flip, rotation, lineShape, fillBrush, shadow 요소가 없습니다.

```xml
<!-- ❌ WRONG — equation은 shape common이 없음 -->
<hp:equation><hp:offset .../><hp:orgSz .../></hp:equation>

<!-- ✅ CORRECT — sz + pos + outMargin + script만 -->
<hp:equation>
  <hp:sz .../>
  <hp:pos .../>
  <hp:outMargin .../>
  <hp:script>...</hp:script>
</hp:equation>
```

`flowWithText="1"` (도형은 0), `outMargin` left/right=56 (도형은 0 또는 283).
````

````admonish danger title="7. 차트 XML을 content.hpf manifest에 등록하면 안 됨"
`Chart/*.xml` 파일을 manifest에 등록하면 한글이 즉시 충돌합니다. ZIP 파일에만 존재해야 합니다.

```xml
<!-- ❌ WRONG — 한글 크래시 유발 -->
<opf:item id="chart1" href="Chart/chart1.xml" media-type="application/xml"/>

<!-- ✅ CORRECT — Chart/*.xml은 ZIP에만 존재, content.hpf에 등록하지 않음 -->
```
````

````admonish danger title="8. 차트 데이터에 <c:f> formula 참조가 필수"
`<c:f>` 요소가 없으면 차트가 열리지만 데이터가 표시되지 않습니다(빈 차트). 더미 수식이라도 반드시 포함해야 합니다.

```xml
<!-- ❌ WRONG — 차트 열리지만 데이터 표시 안 됨 -->
<c:cat><c:strRef><c:strCache>...</c:strCache></c:strRef></c:cat>

<!-- ✅ CORRECT — 더미 formula라도 반드시 포함 -->
<c:cat>
  <c:strRef>
    <c:f>Sheet1!$A$2:$A$5</c:f>
    <c:strCache>...</c:strCache>
  </c:strRef>
</c:cat>
```

한글은 `<c:f>` 존재 여부를 cache 데이터 읽기의 전제조건으로 사용합니다.
````

````admonish danger title="9. 차트 시리즈 이름 <c:tx>는 직접값만 허용"
`<c:strRef>` 방식으로 시리즈 이름을 지정하면 한글이 충돌합니다. `<c:v>`로 직접 값을 지정해야 합니다.

```xml
<!-- ❌ WRONG — 한글 크래시 -->
<c:tx><c:strRef><c:strCache>...</c:strCache></c:strRef></c:tx>

<!-- ✅ CORRECT -->
<c:tx><c:v>시리즈명</c:v></c:tx>
```
````

````admonish danger title="10. <hp:chart> 요소에 dropcapstyle 속성 필수"
`dropcapstyle="None"` 속성이 없으면 한글이 충돌합니다. 또한 `horzRelTo`는 `"PARA"`가 아닌 `"COLUMN"`이어야 합니다.

```xml
<!-- ✅ CORRECT -->
<hp:chart dropcapstyle="None" horzRelTo="COLUMN" .../>
```
````

```admonish warning title="11. TextBox(hp:rect) 인코딩 6가지 핵심 규칙"
글상자를 인코딩할 때 지켜야 할 6가지 규칙입니다.

1. **모서리 좌표는 `hc:` 네임스페이스**: `<hc:pt0>` ~ `<hc:pt3>` (`hp:pt0` 아님)
2. **요소 순서**: shape-common → drawText → caption → hc:pt0-3 → sz → pos → outMargin → shapeComment
3. **lastWidth = 전체 width**: margin을 차감하지 않음
4. **Shadow alpha = 178**: 기본값 0이 아님
5. **shapeComment 필수**: `<hp:shapeComment>사각형입니다.</hp:shapeComment>`
6. **Shape run 후 `<hp:t/>` marker 필수**: 모든 shape 포함 run에 빈 `<hp:t/>` 추가
```

````admonish warning title="12. 다각형 꼭짓점은 첫 번째를 마지막에 반복해야 닫힘"
한글은 path를 자동으로 닫지 않습니다. 첫 꼭짓점을 마지막에 반복하지 않으면 삼각형이 2변만 표시됩니다.

```xml
<!-- ❌ WRONG — 삼각형이 2변만 표시됨 -->
<hc:pt x="0" y="100"/>
<hc:pt x="50" y="0"/>
<hc:pt x="100" y="100"/>

<!-- ✅ CORRECT — 첫 꼭짓점을 마지막에 반복 -->
<hc:pt x="0" y="100"/>
<hc:pt x="50" y="0"/>
<hc:pt x="100" y="100"/>
<hc:pt x="0" y="100"/>
```
````

````admonish warning title="13. VHLC/VOHLC 주식 차트는 4축 combo layout 필수"
barChart와 stockChart가 catAx를 공유하는 3축 레이아웃을 사용하면 한글 렌더링이 깨집니다. 각 차트 타입은 자체 축 쌍(catAx+valAx)을 가져야 합니다.

```xml
<!-- ❌ WRONG — 3축 layout (catAx 공유) → 한글 렌더링 깨짐 -->
<c:barChart>...<c:axId val="1"/><c:axId val="3"/></c:barChart>
<c:stockChart>...<c:axId val="1"/><c:axId val="2"/></c:stockChart>
<c:catAx><c:axId val="1"/>...</c:catAx>

<!-- ✅ CORRECT — OOXML 표준 4축 combo layout -->
<c:barChart>...<c:axId val="3"/><c:axId val="4"/></c:barChart>
<c:stockChart>...<c:axId val="1"/><c:axId val="2"/></c:stockChart>
<c:catAx><c:axId val="1"/><c:crossAx val="2"/>...</c:catAx>
<c:valAx><c:axId val="2"/><c:crossAx val="1"/>...</c:valAx>
<c:catAx><c:axId val="3"/><c:crossAx val="4"/><c:delete val="1"/>...</c:catAx>
<c:valAx><c:axId val="4"/><c:crossAx val="3"/><c:crosses val="max"/>...</c:valAx>
```

secondary catAx는 `delete="1"`로 숨깁니다.
````

---

## 페이지/레이아웃 (Page & Layout)

````admonish danger title="14. landscape 속성값이 스펙과 반대"
실제 한글 파일의 `landscape` 속성값은 KS X 6101 스펙과 **반대**입니다. width/height 비교로 가로/세로를 추론하지 마세요.

| 값 | KS X 6101 스펙 | 한글 실제 동작 |
|---|---|---|
| `WIDELY` | 가로(landscape) | 세로(portrait) |
| `NARROWLY` | 세로(portrait) | 가로(landscape) |

또한 width/height는 항상 세로 기준으로 유지해야 합니다 (예: A4 = 210x297). 한글이 내부적으로 회전 처리합니다.

```rust
// ❌ WRONG — width/height 교환 시 이중 회전 발생
let landscape = PageSettings {
    width: HwpUnit::from_mm(297.0).unwrap(),
    height: HwpUnit::from_mm(210.0).unwrap(),
    ..PageSettings::a4()
};

// ✅ CORRECT — landscape: true, 치수는 세로 기준 유지
let landscape = PageSettings {
    landscape: true,
    ..PageSettings::a4()
};
```
````

````admonish warning title="15. colPr self-closing 태그와 ctrl 요소 순서"
`build_col_pr_xml`은 self-closing `<hp:colPr ... />`를 생성합니다. `</hp:colPr>`를 검색하면 매칭에 실패합니다.

```rust
// ❌ WRONG — self-closing 태그를 놓침
xml.find("</hp:colPr>")

// ✅ CORRECT — 양쪽 형태 모두 매칭
xml.find("<hp:colPr")
```

secPr 내 ctrl 요소 순서: `secPr → colPr → header → footer → pageNum`
````

```admonish warning title="16. Modern 스타일셋의 개요 8/9/10 paraPr 인덱스는 비순차"
Modern(22) 스타일셋에서 개요 8/9/10의 paraPr 인덱스는 순차적이지 않습니다. 순차적이라고 가정하면 잘못된 스타일이 적용됩니다.

| 스타일 | Style ID | paraPr 그룹 |
|---|---|---|
| 개요 8 | 9 | 18 |
| 개요 9 | 10 | 16 |
| 개요 10 | 11 | 17 |

Modern 스타일셋에서 사용자 paraShape는 인덱스 20부터 시작합니다.
```

````admonish danger title="17. MasterPage XML은 prefix 없는 루트 + 15개 xmlns 전체 선언 필수"
`<hm:masterPage>` 형태로 prefix를 쓰거나 xmlns 선언이 누락되면 한글이 즉시 충돌합니다.

```xml
<!-- ❌ WRONG — 한글 크래시 (예기치 않게 종료) -->
<hm:masterPage xmlns:hp="..." xmlns:hm="...">
  <hm:subList>...</hm:subList>
</hm:masterPage>

<!-- ✅ CORRECT — prefix 없는 루트 + 15개 xmlns 전체 선언 + hp:subList -->
<masterPage xmlns="http://www.hancom.co.kr/hwpml/2011/master"
            xmlns:hp="..." xmlns:hh="..." xmlns:hc="..." ...>
  <hp:subList id="" textDirection="HORIZONTAL" ...>
    ...
  </hp:subList>
</masterPage>
```

3가지 핵심 규칙:
1. 루트 요소는 `<masterPage>` (prefix 없음, `<hm:masterPage>` 아님)
2. header/section과 동일한 15개 namespace 전부 선언 필수
3. `<hp:subList>` 사용 (`<hm:subList>` 아님)
````

---

## 필드/참조 (Fields & References)

````admonish warning title="18. paraPr 당 switch가 여러 개일 수 있음"
실제 한글 파일에서는 `<hh:paraPr>` 당 2개 이상의 `<hp:switch>`가 있습니다 (예: 제목용 하나, 여백/줄간격용 하나). 스키마는 `Option<HxSwitch>`가 아닌 `Vec<HxSwitch>`를 사용해야 합니다.

```rust
// ❌ WRONG
switches: Option<HxSwitch>

// ✅ CORRECT
switches: Vec<HxSwitch>
```
````

````admonish warning title="19. 하이퍼링크는 fieldBegin/fieldEnd 패턴 (<hp:hyperlink> 없음)"
`<hp:hyperlink>` 요소는 HWPX에 존재하지 않습니다. KS X 6101의 field pair 패턴을 사용해야 합니다.

```xml
<!-- ❌ WRONG — 이런 요소 없음 -->
<hp:hyperlink href="...">...</hp:hyperlink>

<!-- ✅ CORRECT — KS X 6101 field pair -->
<hp:run charPrIDRef="0">
  <hp:ctrl>
    <hp:fieldBegin type="HYPERLINK" fieldid="0">
      <hp:parameters cnt="4">
        <hp:stringParam name="Path">https://url.com</hp:stringParam>
        <hp:stringParam name="Category">HWPHYPERLINK_TYPE_URL</hp:stringParam>
        <hp:stringParam name="TargetType">HWPHYPERLINK_TARGET_DOCUMENT_DONTCARE</hp:stringParam>
        <hp:stringParam name="DocOpenType">HWPHYPERLINK_JUMP_NEWTAB</hp:stringParam>
      </hp:parameters>
    </hp:fieldBegin>
  </hp:ctrl>
  <hp:t>링크 텍스트</hp:t>
  <hp:ctrl><hp:fieldEnd beginIDRef="0" fieldid="0"/></hp:ctrl>
</hp:run>
```
````

````admonish warning title="20. 각주/미주는 같은 문단의 인라인 Run으로 삽입"
각주/미주를 별도 문단으로 만들면 각주 번호가 단독 줄에 표시됩니다. 반드시 같은 문단의 Run에 포함해야 합니다.

```rust
// ❌ WRONG — 별도 문단으로 만들면 "1)"이 단독 줄에 표시됨
paras.push(p("본문 텍스트."));
paras.push(ctrl_para(Control::footnote(notes), CS_NORMAL, PS_JUSTIFY));

// ✅ CORRECT — 같은 문단의 Run에 포함
paras.push(Paragraph::with_runs(
    vec![
        Run::text("본문 텍스트.", CharShapeIndex::new(0)),
        Run::control(Control::footnote(notes), CharShapeIndex::new(0)),
    ],
    ParaShapeIndex::new(0),
));
```
````

````admonish danger title="21. 날짜/시간 필드는 type=SUMMERY (오타 주의)"
한글 내부에서 14년간 유지된 오타입니다. `"DATE"` 또는 `"TIME"`을 쓰면 아무것도 표시되지 않습니다.

```xml
<!-- ❌ WRONG — 한글이 인식하지 않음 (빈 필드) -->
<hp:fieldBegin type="DATE" ...>
<hp:fieldBegin type="TIME" ...>

<!-- ✅ CORRECT — "Summary"의 오타 "SUMMERY" 사용 -->
<hp:fieldBegin type="SUMMERY" fieldid="628321650" ...>
  <hp:parameters cnt="3" name="">
    <hp:integerParam name="Prop">8</hp:integerParam>
    <hp:stringParam name="Command">$modifiedtime</hp:stringParam>
    <hp:stringParam name="Property">$modifiedtime</hp:stringParam>
  </hp:parameters>
</hp:fieldBegin>
```

Command 매핑: `$modifiedtime`=날짜, `$createtime`=시간, `$author`=작성자, `$lastsaveby`=최종수정자.
CLICK_HERE와의 차이: `Prop=8` (CLICK_HERE는 9), `fieldid=628321650` (CLICK_HERE는 627272811).
````

````admonish warning title="22. 본문 쪽번호는 <hp:autoNum> 사용 (fieldBegin 아님)"
`type="PAGE_NUM"`은 유효한 fieldBegin 타입이 아닙니다. 본문에 쪽번호를 삽입할 때는 autoNum 메커니즘을 사용해야 합니다.

```xml
<!-- ❌ WRONG — PAGE_NUM은 존재하지 않는 타입 -->
<hp:fieldBegin type="PAGE_NUM" ...>

<!-- ✅ CORRECT — autoNum 메커니즘 사용 -->
<hp:ctrl>
  <hp:autoNum num="1" numType="PAGE">
    <hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>
  </hp:autoNum>
</hp:ctrl>
```

3가지 쪽번호 메커니즘 혼동 주의:
- `<hp:pageNum>`: secPr 내 ctrl (머리글/바닥글 자동 배치)
- `<hp:autoNum numType="PAGE">`: 본문 텍스트 인라인 삽입
- ~~`type="PAGE_NUM"` fieldBegin~~: 존재하지 않음
````

````admonish warning title="23. page_break는 문단 속성으로 직접 인코딩"
`pageBreak` 속성을 하드코딩된 0으로 설정하면 페이지 나누기가 동작하지 않습니다.

```rust
// ❌ WRONG (하드코딩)
page_break: 0,

// ✅ CORRECT — para.page_break 필드에서 읽기
page_break: u32::from(para.page_break),
```

`encoder/section.rs`의 `build_paragraph()`에서 `pageBreak` 속성을 `para.page_break` 필드로부터 읽어야 합니다.
````

---

## 스타일 (Styles)

````admonish warning title="24. breakNonLatinWord는 반드시 KEEP_WORD"
`BREAK_WORD`를 사용하면 양쪽 정렬(justify) 텍스트에서 글자 사이 공간이 균등 분배되어 글자가 비정상적으로 퍼집니다. 한글 기본값인 `KEEP_WORD`를 사용해야 합니다.

```rust
// ❌ WRONG — 양쪽 정렬 시 글자 사이 공간 균등 분배 → 퍼짐 현상
break_non_latin_word: "BREAK_WORD"

// ✅ CORRECT — 한글 기본값, 단어 단위 공간 분배 → 자연스러운 정렬
break_non_latin_word: "KEEP_WORD"
```

위치: `crates/hwpforge-smithy-hwpx/src/encoder/header.rs` `build_para_pr()`
````

````admonish warning title="25. 화살표 도형은 반드시 EMPTY_* 형태 사용"
KS X 6101 스키마에는 `FILLED_DIAMOND`, `FILLED_CIRCLE`, `FILLED_BOX`가 유효한 값으로 정의되어 있지만, 실제 한글은 이를 인식하지 못합니다. `EMPTY_*` 형태와 `headfill`/`tailfill` 속성(0 또는 1)으로 채움 여부를 제어해야 합니다.

```xml
<!-- ❌ WRONG — 한글이 인식하지 않음 (화살촉 안 보임) -->
<hp:lineShape headStyle="FILLED_DIAMOND" headfill="1" .../>

<!-- ✅ CORRECT — EMPTY_* + headfill="1" = 채워진 다이아몬드 -->
<hp:lineShape headStyle="EMPTY_DIAMOND" headfill="1" .../>

<!-- ✅ CORRECT — EMPTY_* + headfill="0" = 빈 다이아몬드 -->
<hp:lineShape headStyle="EMPTY_DIAMOND" headfill="0" .../>
```

적용 대상: `EMPTY_DIAMOND`, `EMPTY_CIRCLE`, `EMPTY_BOX`
비기하 도형은 그대로: `NORMAL`, `ARROW`, `SPEAR`, `CONCAVE_ARROW` (fill 속성 무관)
````

````admonish warning title="26. DropCapStyle은 PascalCase (도형 레벨 속성)"
`DropCapStyle`은 문단 속성이 아니라 도형(`AbstractShapeObjectType`)의 속성입니다. 값은 SCREAMING_SNAKE_CASE가 아닌 PascalCase를 사용해야 합니다.

```xml
<!-- ❌ WRONG — SCREAMING_SNAKE_CASE -->
dropcapstyle="DOUBLE_LINE"

<!-- ✅ CORRECT — PascalCase (KS X 6101 XSD 준수) -->
dropcapstyle="DoubleLine"
```

유효한 값: `None`, `DoubleLine`, `TripleLine`, `Margin`
````

---

## 라이브러리 호환성 (Library Compatibility)

```admonish info title="27. HWP5 TagID에 +16 오프셋"
Section 레코드는 공식 스펙보다 +16 오프셋을 가집니다.

| 항목 | 스펙 값 | 실제 값 |
|---|---|---|
| `PARA_HEADER` | `0x32` (50) | `0x42` (66) |

자세한 내용은 `.docs/research/SPEC_VS_REALITY.md`를 참조하세요.
```

```admonish info title="28. Foundation 의존성은 최소화"
Foundation은 의존성 그래프의 루트입니다. Foundation을 수정하면 모든 crate가 재빌드됩니다. 불필요한 의존성을 추가하지 마세요.

Phase 0 Oracle 리뷰에서 사용하지 않는 의존성 3개를 제거한 사례가 있습니다.
```

````admonish info title="29. schemars 1.x: schema_name() 반환 타입 변경"
schemars 0.8에서 1.x로 업그레이드하면 `schema_name()`의 반환 타입이 변경되었습니다.

```rust
// ❌ WRONG (schemars 0.8 API)
fn schema_name() -> String { "MyType".to_owned() }

// ✅ CORRECT (schemars 1.x API)
fn schema_name() -> Cow<'static, str> { Cow::Borrowed("MyType") }
```
````

````admonish info title="30. quick-xml 0.39: unescape() 제거"
quick-xml 0.36에서 0.39로 업그레이드하면 `unescape()` API가 제거되었습니다. 또한 `Event::GeneralRef` variant가 추가되어 exhaustive match에서 처리해야 합니다.

```rust
// ❌ WRONG (quick-xml 0.36 API — 0.39에서 제거됨)
let text = event.unescape()?;

// ✅ CORRECT (quick-xml 0.39)
let text = reader.decoder().decode(event.as_ref())?;
```
````

---

## 요약 체크리스트

구현 전에 확인하세요:

- [ ] 색상 값에 `Color::from_rgb()` 사용 (BGR 혼동 방지)
- [ ] 기하 좌표에 `hc:` 네임스페이스 사용 (선/다각형/글상자 모서리)
- [ ] 차트 XML을 `content.hpf`에 등록하지 않음
- [ ] 차트 데이터에 `<c:f>` 더미 formula 포함
- [ ] 차트 시리즈 이름에 `<c:v>` 직접값 사용
- [ ] 주식 차트에 4축 combo layout 사용
- [ ] 가로 방향에 `landscape: true` 플래그 사용 (width/height 교환 금지)
- [ ] MasterPage XML에 prefix 없는 루트 + 15개 xmlns 선언
- [ ] 날짜/시간 필드에 `type="SUMMERY"` (오타 포함)
- [ ] 본문 쪽번호에 `<hp:autoNum>` 사용 (fieldBegin 아님)
- [ ] 화살표에 `EMPTY_*` + `headfill`/`tailfill` 조합 사용
- [ ] `DropCapStyle`에 PascalCase 값 사용
- [ ] `breakNonLatinWord`를 `KEEP_WORD`로 설정
- [ ] 다각형 꼭짓점 목록 마지막에 첫 꼭짓점 반복
- [ ] 각주/미주를 같은 문단의 Run에 인라인으로 삽입
