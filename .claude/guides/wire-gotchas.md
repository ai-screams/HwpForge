# HWPX/HWP5 Wire Gotchas (#1~37)

> 이 파일은 CLAUDE.md 로딩 맵에서 필요 시 로드된다 (자동 로드 아님).
> **상세 내용 (코드 예제 포함)**: `.docs/references/gotchas.md` (43항목)

1. HWP5 TagID +16 오프셋 — `PARA_HEADER` = 0x42 (66), not 0x32 (50)
2. landscape 스펙 반전 — `WIDELY`=세로, `NARROWLY`=가로. width/height 교환 금지
3. 기하 좌표는 모두 `hc:` namespace (`hp:` 사용 시 한글 parse error)
4. TextBox = `hp:rect` + `hp:drawText` (control 요소 아님). 요소 순서/shapeComment 필수
5. Chart: manifest 등록 금지, `<c:f>` 필수, `<c:tx>`는 직접값만, `dropcapstyle="None"` 필수
6. paraPr 당 `Vec<HxSwitch>` (NOT `Option`) — 2개 이상 switch 가능
7. Equation: shape common 블록 없음 (`flowWithText="1"`, `outMargin` left/right=56)
8. colPr self-closing 태그 — `xml.find("<hp:colPr")` 로 매칭
9. Polygon 꼭짓점 닫힘 — 첫 꼭짓점을 마지막에 반복 필수
10. `breakNonLatinWord` = `KEEP_WORD` (BREAK_WORD 시 글자 퍼짐)
11. Field: 하이퍼링크=`fieldBegin/End`, 날짜=`type="SUMMERY"` (오타), 쪽번호=`autoNum`
12. 각주/미주: 같은 문단의 inline Run에 포함 (별도 문단 금지). HWP5 decode 시 ParaText `0x11` 마커(ctrl_id `fn`/`en`)를 `ControlRef`로 승격해야 inline 유지 — 안 하면 문단 꼬리로 drain되어 마커가 단독 줄에 표시
13. Style: 개요 8/9/10 paraPr 비순차(18/16/17), DropCapStyle은 PascalCase
14. ArrowType: `EMPTY_*` + `headfill` 조합만 (FILLED_* 무시됨)
15. MasterPage: prefix 없는 `<masterPage>` 루트, 15개 xmlns, `<hp:subList>`
16. schemars 1.x: `Cow<'static, str>` 반환. quick-xml 0.41: `decoder().decode()` 사용
17. `page_break`: `u32::from(para.page_break)` — hardcoded 0 금지
18. Flip은 `rotMatrix`에 인코딩 — scaMatrix/transMatrix는 identity 유지
19. `fillBrush`는 xs:choice — winBrush/gradation/imgBrush 중 하나만
20. Rotation: 정수 degrees + CCW 방향 + 중심 이동 보정 필수
21. PatternType `BACK_SLASH`/`SLASH` 스펙 반전 — Display/FromStr에서 스왑
22. 패턴 채우기: `hatchStyle` 속성 필수 (없으면 솔리드로 렌더링)
23. fieldid = ctrl_id ASCII magic constant (`%xrf`/`%clk`/`%smr`/`%pat`) — type tag, instance ID 아님
24. CROSSREF wire = 8-param Hancom-canonical (`Fiexde`/`Prop`/`Command` 포함, 5-param spec form 금지)
25. cross-ref target element (endNote/footNote/figure/table) 에 `instId` attribute 필수
26. Bookmark Contents reference 는 SpanStart/SpanEnd 책갈피 필요 (Point 는 본문 없음 → `?`)
27. ContentType 의미는 RefType-상대적 (Bookmark+Contents = 책갈피 이름, Figure+Contents = 캡션 본문) — invented enum 금지
28. 도형/글상자 텍스트 **세로정렬 = HWP5 ListHeader 속성 bits 5-6** (`(props>>5)&0x03`, 0/1/2=Top/Center/Bottom). 표 셀 디코드(`smithy-hwp5/src/decoder/section/mod.rs`)가 ground truth — openhwp `(props>>2)`는 우리 wire와 불일치
29. 도형 drawText `<hp:subList textWidth/textHeight>` = **`0`이 Hancom-정답** (렌더러는 `<hp:sz>`−`<hp:textMargin>`(기본 283)으로 텍스트 영역 계산). 계산값으로 "고치지" 말 것 — 한컴 fixture·KS X 6101 샘플 141 확인
30. 누름틀(ClickHere) 본문 = `fieldBegin`~`fieldEnd` 사이 평범한 `<hp:t>` (미채움 = 힌트와 동일 문자열). `display_text` 빈 문자열 = 미채움/모호 sentinel — patch 슬롯·redact·fill 이 전부 ClickHere-gated 로 이 불변식 공유. 한컴 재저장은 라벨 run 을 필드 run 에 병합 → run 에 `<hp:t>` 1개일 때만 본문 무모호 귀속 (HxRun 은 자식 순서 미보존)
31. HWPX `hp:pos` 음수 offset = **u32 랩어라운드 십진 문자열** (`horzOffset="4294965029"` = −2267) / HWP5 는 signed i32 — 파서는 u32 파싱 후 i32 캐스트 (i32 직파싱은 오버플로우)
32. 한컴 저작 정규화 3종: API 가 쓴 음수 vertOffset 은 **재저장에서 0 클램프** · **드래그는 앵커 재지정**(가장 가까운 위 문단 + 작은 양수 offset — 음수를 안 씀) · 음수 offset 은 **개체 속성 대화상자 직접 입력만** 저작 가능 (corpus 음수 값의 출처)
33. HWP5 개체 공통 속성 word(표 70) 비트 배치 = `WIRE_SPEC.md §22.5` (bit0 글자취급 · 3-4 vertRelTo · 8-9 horzRelTo · 21-23 wrap · 24-25 flow — **글자취급 bit0 은 TextBox 컨텍스트에서도 유효**, 컨텍스트별 하드코딩 관례 금지). 앵커 축 census 는 반드시 byte-ground 디코드 후에 — 관례-필터 데이터는 동어반복
34. 각주/미주 번호 머리 = `[autoNum head run, " " spacer run]` 쌍 주입 + 디코더 **대칭 쌍 드롭** (본문 run 불변 — 문자 하나만 붙여도 치환 마커 오염). 디코더는 FOOTNOTE/ENDNOTE autoNum 을 Core 로 **안 올림** → 편집 재인코드에서 "이미 있음" 판정 불가 (한컴 native 는 autoNum+본문 fused run 이라 쌍 시그니처 비적중)
35. 인코더 전체-run 치환 placeholder(HWPHL/HWPFD/HWPBM 등 13종) = **run 직렬화 문자열 전체가 치환 키** — titleMark 등 후주입으로 run 을 바꾸면 내부 마커가 XML 로 유출. 판정은 등록 키와 **정확 일치**로만 (substring 은 정상 텍스트 "0"·"hp" 오탐)
36. `BeginNum.footnote/endnote` 는 `<hp:startNum>` wire 에 **속성 자체가 없음** — 디코더가 1 을 합성 (첫 섹션만 header `<hh:beginNum>` 실값 병합). 재시작 신호로 읽으면 공개 왕복에서 각주 번호 1,2,3→1,2,1 리셋
37. `hp:newNum@num` = **xs:integer** (0 유효 wire — verbatim 캐리) / `xs:positiveInteger` 는 footNotePr `numbering@newNum` 쪽. 두 XSD 를 혼동해 0 을 거르면 wire-캐시 의미 분열

---
