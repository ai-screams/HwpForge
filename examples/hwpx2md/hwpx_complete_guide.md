HWPX 문서 구조 완전 가이드

![image1](BinData/image1)

HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 순수 Rust 라이브러리입니다. 프로젝트 마스코트인 오리너구리(Platypus)는 HWPX 포맷의 독특한 특성을 상징합니다 — XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.

1. HWPX 문서 포맷이란?

HWPX는 대한민국 국가표준 KS X 6101(footnote: KS X 6101: 한국산업표준(Korean Industrial Standards)에서 제정한 문서 파일 형식 표준. 2014년 최초 제정, 2021년 개정.)에 정의된 XML 기반 문서 포맷입니다. ZIP 컨테이너 안에 여러 XML 파일이 구조화되어 저장됩니다.

상세 사양은 [한국정보통신기술협회(TTA)](https://www.tta.or.kr) 홈페이지에서 확인할 수 있습니다.

1. ZIP 컨테이너 파일 구성

HWPX 파일은 확장자가 .hwpx인 ZIP 아카이브입니다. 내부에는 다음과 같은 XML 파일들이 포함됩니다:

| 파일 경로                                                                               | 설명                           | Media-Type      |
| --------------------------------------------------------------------------------------- | ------------------------------ | --------------- |
| META-INF/manifest.xml                                                                   | 패키지 매니페스트 (파일 목록)  | text/xml        |
| Contents/content.hpf                                                                    | 콘텐츠 목차 (OPF)              | application/xml |
| Contents/header.xml                                                                     | 스타일 정의 (폰트, 문단, 글자) | application/xml |
| Contents/section0.xml                                                                   | 본문 첫 번째 구획 (paragraphs) | application/xml |
| Contents/section1.xml                                                                   | 본문 두 번째 구획 (선택적)     | application/xml |
| BinData/ — 이미지, OLE 등 바이너리 데이터 폴더 (Content.hpf에 등록, Chart XML은 미등록) |                                |                 |

1. 섹션(Section) 구조

HWPX 문서는 하나 이상의 섹션으로 구성됩니다. 각 섹션은 독립적인 페이지 설정(용지 크기, 여백, 방향)을 가질 수 있어, 세로 페이지와 가로 페이지를 하나의 문서에 혼합할 수 있습니다.

각 섹션의 XML은 <hp:sec> 루트 아래 <hp:p>(문단) 요소들로 구성됩니다. 문단 안에는 <hp:run>(텍스트 런), <hp:ctrl>(컨트롤), <hp:tbl>(표) 등이 포함됩니다.

1. header.xml 스타일 시스템

header.xml에는 문서 전체의 스타일 정의가 담깁니다: fontface(폰트), charShape(글자 모양), paraShape(문단 모양). 본문의 각 요소는 인덱스(IDRef)로 이 정의를 참조합니다.

스타일 정의 인덱스는 0부터 시작하며, Modern 스타일셋 기준으로 기본 charShape 7개, paraShape 20개가 자동 생성됩니다(footnote: 한글 2022(Modern 스타일셋)의 기본 스타일: charShape 0-6 (바탕~개요10), paraShape 0-19 (바탕~개요10). 사용자 정의 스타일은 이후 인덱스부터 시작합니다.).

이 문서는 HwpForge 라이브러리로 생성되었으며, 4개 섹션에 걸쳐 문서 포맷의 각 요소를 실제로 사용하면서 설명합니다.

<!-- hwpforge:section -->

텍스트 서식 시스템

1. 문단 정렬 (Paragraph Alignment)

양쪽 정렬(Justify): 본문에서 가장 일반적으로 사용되는 정렬입니다. 양쪽 여백에 맞춰 글자 간격이 자동 조절됩니다.

가운데 정렬(Center): 제목이나 캡션에 주로 사용합니다.

왼쪽 정렬(Left): 코드나 목록에 적합합니다.

오른쪽 정렬(Right): 날짜, 서명 등에 사용합니다.

배분 정렬(Distribute): 글자를 균등하게 분배합니다.

1. 덧말 (Dutmal / Ruby Text)

덧말은 한자 위나 아래에 한글 읽기를 표시하는 기능입니다:

위쪽 덧말: 大韓民國(대한민국) 아래쪽 덧말: 漢字(한자) 오른쪽 덧말: 情報(정보)

1. 글자겹침 (Compose)

글자겹침 기능: 12 (숫자 1과 2를 겹침)

1. 필드 (Field)

누름틀(ClickHere):

날짜 필드(Date):

쪽 번호 필드(autoNum): 현재 쪽

1. 미주 (Endnote)

글자 모양(charShape)은 폰트, 크기, 색상, 굵기, 기울임, 밑줄, 취소선 등을 정의합니다(endnote: charShape 속성 목록: height(크기), textColor(색상), bold(굵기), italic(기울임), underlineType(밑줄), strikeoutShape(취소선), emphasis(강조점), ratio(장평), spacing(자간), relSz(상대크기), offset(세로위치), useKerning(커닝), useFontSpace(폰트 자간).).

1. 메모 (Memo)

이 문단에는 검토 메모가 첨부되어 있습니다.

1. 상호참조 (CrossRef)

HWPX 문서 정의는 섹션 1의 쪽을 참조하세요. ZIP 파일 구조는 쪽에 설명되어 있습니다.

1. 글자 서식 변화 시연

기본 굵게 파랑 기울임 녹색 작은 글씨 제목 크기 회색 워터마크

<!-- hwpforge:section -->

도형과 그래픽 요소

이 섹션은 가로(landscape) 방향이며, Gutter 10mm가 적용되어 있습니다. HWPX의 다양한 도형 요소를 시연합니다.

3.1 선 (Line)

실선 (기본):

점선 + 화살표:

쇄선(DashDot) 빨강:

3.2 타원 (Ellipse)

타원 내부 텍스트

3.3 다각형 (Polygon)

삼각형 (그라디언트 채우기):

오각형 (패턴 채우기):

3.4 호 (Arc) — 3가지 타입

Normal (열린 호):

Pie (부채꼴):

Chord (활꼴):

3.5 곡선 (Curve)

베지어 S자 곡선:

3.6 연결선 (ConnectLine)

양방향 다이아몬드 화살표:

3.7 글상자 (TextBox)

이것은 글상자(TextBox) 안의 문단입니다. HWPX에서 글상자는 <hp:rect> + <hp:drawText> 구조로 인코딩됩니다. 별도의 Control 요소가 아닌 도형 객체입니다.

3.8 도형 스타일 — 회전/뒤집기

타원 45도 회전:

타원 수평 뒤집기:

<!-- hwpforge:section -->

차트, 수식, 고급 기능

4.1 수식 (Equation — HancomEQN)

HWPX의 수식은 HancomEQN 스크립트 형식을 사용합니다. MathML이 아닌 자체 문법입니다:

분수:

제곱근:

적분:

행렬:

4.2 차트 (Chart — OOXML)

HWPX는 OOXML(Office Open XML) 차트 형식을 사용합니다. Chart XML은 ZIP 내 별도 파일로 저장되며, content.hpf 매니페스트에는 등록하지 않습니다.

세로막대 차트 (Column, Clustered):

원형 차트 (Pie):

꺾은선 차트 (Line):

분산형 차트 (Scatter):

4.3 고급 표 서식

표는 col_span으로 셀 병합, background로 배경색 지정이 가능합니다:

| HWPX 요소 분류표 |
| ---------------- |
| 분류             |
| 구조             |
| 서식             |
| 객체             |
| 도형             |
| 주석             |
| 필드             |

4.4 페이지 테두리 (PageBorderFill) + BeginNum

이 섹션에는 페이지 테두리(borderFillIDRef=3, 검은 실선)가 설정되어 있으며, 페이지 번호는 1부터 새로 시작합니다.

4.5 종합 요약

이 문서는 HwpForge 라이브러리의 전체 API를 사용하여 생성되었습니다. 4개 섹션에 걸쳐 다음 기능들을 시연했습니다:

구조: Document, Section, Paragraph, Run, Table, Image(Store)

섹션: Header, Footer, PageNumber, ColumnSettings, Visibility, LineNumberShape

섹션: PageBorderFill, MasterPage, BeginNum, Gutter, Landscape

도형: Line, Ellipse, Polygon, Arc, Curve, ConnectLine, TextBox

스타일: ShapeStyle (rotation, flip, fill, arrow), Caption (4방향)

채우기: Solid, Gradient (Linear), Pattern (HorizontalLine)

차트: Column, Pie, Line, Scatter (OOXML 형식)

수식: fraction, root, integral, matrix (HancomEQN)

텍스트: Dutmal (3방향), Compose (글자겹침)

참조: Bookmark (Point/Span), CrossRef, Hyperlink

필드: ClickHere, Date, PageNum

주석: Footnote, Endnote, Memo, IndexMark

정렬: Left, Center, Right, Justify, Distribute

스타일스토어: Font, CharShape(8종), ParaShape(5종), BorderFill(4종), Numbering, Tab

=== HWPX 문서 구조 완전 가이드 끝 ===
