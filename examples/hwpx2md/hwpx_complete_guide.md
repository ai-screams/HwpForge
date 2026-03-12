**HWPX 문서 구조 완전 가이드**

![image1](images/image1.png)

HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 **순수 Rust 라이브러리**입니다. 프로젝트 마스코트인 <strong>오리너구리(Platypus)</strong>는 HWPX 포맷의 독특한 특성을 상징합니다 — XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.

<strong>1. HWPX 문서 포맷이란?</strong>

HWPX는 대한민국 국가표준 KS X 6101[^1]에 정의된 XML 기반 문서 포맷입니다. ZIP 컨테이너 안에 여러 XML 파일이 구조화되어 저장됩니다.

상세 사양은 [한국정보통신기술협회(TTA)](https://www.tta.or.kr) 홈페이지에서 확인할 수 있습니다.

**2. ZIP 컨테이너 파일 구성**

HWPX 파일은 확장자가 .hwpx인 ZIP 아카이브입니다. 내부에는 다음과 같은 XML 파일들이 포함됩니다:

<table>
<tbody>
<tr>
  <td><strong>파일 경로</strong></td>
  <td><strong>설명</strong></td>
  <td><strong>Media-Type</strong></td>
</tr>
<tr>
  <td>META-INF/manifest.xml</td>
  <td>패키지 매니페스트 (파일 목록)</td>
  <td>text/xml</td>
</tr>
<tr>
  <td>Contents/content.hpf</td>
  <td>콘텐츠 목차 (OPF)</td>
  <td>application/xml</td>
</tr>
<tr>
  <td>Contents/header.xml</td>
  <td>스타일 정의 (폰트, 문단, 글자)</td>
  <td>application/xml</td>
</tr>
<tr>
  <td>Contents/section0.xml</td>
  <td>본문 첫 번째 구획 (paragraphs)</td>
  <td>application/xml</td>
</tr>
<tr>
  <td>Contents/section1.xml</td>
  <td>본문 두 번째 구획 (선택적)</td>
  <td>application/xml</td>
</tr>
<tr>
  <td colspan="3"><em>BinData/ — 이미지, OLE 등 바이너리 데이터 폴더 (Content.hpf에 등록, Chart XML은 미등록)</em></td>
</tr>
</tbody>
</table>

**3. 섹션(Section) 구조**

HWPX 문서는 하나 이상의 섹션으로 구성됩니다. 각 섹션은 독립적인 페이지 설정(용지 크기, 여백, 방향)을 가질 수 있어, 세로 페이지와 가로 페이지를 하나의 문서에 혼합할 수 있습니다.

각 섹션의 XML은 &lt;hp:sec&gt; 루트 아래 &lt;hp:p&gt;(문단) 요소들로 구성됩니다. 문단 안에는 &lt;hp:run&gt;(텍스트 런), &lt;hp:ctrl&gt;(컨트롤), &lt;hp:tbl&gt;(표) 등이 포함됩니다.

**4. header.xml 스타일 시스템**

header.xml에는 문서 전체의 스타일 정의가 담깁니다: <strong>fontface(폰트)</strong>, <strong>charShape(글자 모양)</strong>, <strong>paraShape(문단 모양)</strong>. 본문의 각 요소는 인덱스(IDRef)로 이 정의를 참조합니다.

스타일 정의 인덱스는 0부터 시작하며, Modern 스타일셋 기준으로 기본 charShape 7개, paraShape 20개가 자동 생성됩니다[^2].

<em>이 문서는 HwpForge 라이브러리로 생성되었으며, 4개 섹션에 걸쳐 문서 포맷의 각 요소를 실제로 사용하면서 설명합니다.</em>

<!-- hwpforge:section -->

**텍스트 서식 시스템**

<strong>1. 문단 정렬 (Paragraph Alignment)</strong>

양쪽 정렬(Justify): 본문에서 가장 일반적으로 사용되는 정렬입니다. 양쪽 여백에 맞춰 글자 간격이 자동 조절됩니다.

가운데 정렬(Center): 제목이나 캡션에 주로 사용합니다.

왼쪽 정렬(Left): 코드나 목록에 적합합니다.

오른쪽 정렬(Right): 날짜, 서명 등에 사용합니다.

배분 정렬(Distribute): 글자를 균등하게 분배합니다.

<strong>2. 덧말 (Dutmal / Ruby Text)</strong>

덧말은 한자 위나 아래에 한글 읽기를 표시하는 기능입니다:

위쪽 덧말: 大韓民國(대한민국) 아래쪽 덧말: 漢字(한자) 오른쪽 덧말: 情報(정보)

<strong>3. 글자겹침 (Compose)</strong>

글자겹침 기능: 12 (숫자 1과 2를 겹침)

<strong>4. 필드 (Field)</strong>

누름틀(ClickHere): 이름을 입력하세요

날짜 필드(Date): ____

쪽 번호 필드(autoNum): 현재 ____쪽

<strong>5. 미주 (Endnote)</strong>

글자 모양(charShape)은 폰트, 크기, 색상, 굵기, 기울임, 밑줄, 취소선 등을 정의합니다[^e1].

<strong>6. 메모 (Memo)</strong>

이 문단에는 검토 메모가 첨부되어 있습니다.<!-- memo(): <strong>검토 의견:</strong> charShape 설명을 표 형태로 정리하면 더 좋겠습니다. 다음 버전에 반영 부탁드립니다. -->

<strong>7. 상호참조 (CrossRef)</strong>

HWPX 문서 정의는 섹션 1의 [HWPX정의]쪽을 참조하세요. ZIP 파일 구조는 [헤더구조]쪽에 설명되어 있습니다.

**8. 글자 서식 변화 시연**

기본 **굵게** 파랑 _기울임 녹색_ 작은 글씨 **제목 크기** 회색 워터마크

<!-- hwpforge:section -->

**도형과 그래픽 요소**

이 섹션은 가로(landscape) 방향이며, Gutter 10mm가 적용되어 있습니다. HWPX의 다양한 도형 요소를 시연합니다.

<strong>3.1 선 (Line)</strong>

실선 (기본):

점선 + 화살표:

쇄선(DashDot) 빨강:

<strong>3.2 타원 (Ellipse)</strong>

타원 내부 텍스트

<strong>3.3 다각형 (Polygon)</strong>

삼각형 (그라디언트 채우기):

오각형 (패턴 채우기):

**3.4 호 (Arc) — 3가지 타입**

Normal (열린 호):

Pie (부채꼴):

Chord (활꼴):

<strong>3.5 곡선 (Curve)</strong>

베지어 S자 곡선:

<strong>3.6 연결선 (ConnectLine)</strong>

양방향 다이아몬드 화살표:

<strong>3.7 글상자 (TextBox)</strong>

이것은 글상자(TextBox) 안의 문단입니다. HWPX에서 글상자는 &lt;hp:rect&gt; + &lt;hp:drawText&gt; 구조로 인코딩됩니다. 별도의 Control 요소가 아닌 도형 객체입니다.

**3.8 도형 스타일 — 회전/뒤집기**

타원 45도 회전:

타원 수평 뒤집기:

<!-- hwpforge:section -->

**차트, 수식, 고급 기능**

<strong>4.1 수식 (Equation — HancomEQN)</strong>

HWPX의 수식은 HancomEQN 스크립트 형식을 사용합니다. MathML이 아닌 자체 문법입니다:

분수:

$\frac{a+b}{c+d}$

제곱근:

$root{2}of{x^{2}+y^{2}}$

적분:

$\int_{0}^{\infty}e^{-x^{2}}dx=\frac{\sqrt{\pi}}{2}$

행렬:

$\left(\begin{pmatrix}a & b \\ c & d\end{pmatrix}\right)$

<strong>4.2 차트 (Chart — OOXML)</strong>

HWPX는 OOXML(Office Open XML) 차트 형식을 사용합니다. Chart XML은 ZIP 내 별도 파일로 저장되며, content.hpf 매니페스트에는 등록하지 않습니다.

세로막대 차트 (Column, Clustered):

<!-- chart -->

원형 차트 (Pie):

<!-- chart -->

꺾은선 차트 (Line):

<!-- chart -->

분산형 차트 (Scatter):

<!-- chart -->

**4.3 고급 표 서식**

표는 col\_span으로 셀 병합, background로 배경색 지정이 가능합니다:

<table>
<tbody>
<tr>
  <td colspan="3"><strong>HWPX 요소 분류표</strong></td>
</tr>
<tr>
  <td><strong>분류</strong></td>
  <td><strong>요소명</strong></td>
  <td><strong>설명</strong></td>
</tr>
<tr>
  <td>구조</td>
  <td>Section, Paragraph, Run</td>
  <td>문서의 기본 골격 (섹션→문단→런)</td>
</tr>
<tr>
  <td>서식</td>
  <td>CharShape, ParaShape, Style</td>
  <td>글자/문단 모양 정의 (header.xml)</td>
</tr>
<tr>
  <td>객체</td>
  <td>Table, Image, TextBox, Chart</td>
  <td>인라인 또는 부동 객체</td>
</tr>
<tr>
  <td>도형</td>
  <td>Line, Ellipse, Polygon, Arc, Curve</td>
  <td>벡터 드로잉 객체 (shape common block)</td>
</tr>
<tr>
  <td>주석</td>
  <td>Footnote, Endnote, Memo, Bookmark</td>
  <td>참조 및 주석 체계</td>
</tr>
<tr>
  <td>필드</td>
  <td>Hyperlink, Field, CrossRef, IndexMark</td>
  <td>fieldBegin/fieldEnd 패턴 인코딩</td>
</tr>
</tbody>
</table>

**4.4 페이지 테두리 (PageBorderFill) + BeginNum**

이 섹션에는 페이지 테두리(borderFillIDRef=3, 검은 실선)가 설정되어 있으며, 페이지 번호는 1부터 새로 시작합니다.

**4.5 종합 요약**

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

<strong>=== HWPX 문서 구조 완전 가이드 끝 ===</strong>

[^1]: KS X 6101: 한국산업표준(Korean Industrial Standards)에서 제정한 문서 파일 형식 표준. 2014년 최초 제정, 2021년 개정.

[^2]: 한글 2022(Modern 스타일셋)의 기본 스타일: charShape 0-6 (바탕\~개요10), paraShape 0-19 (바탕\~개요10). 사용자 정의 스타일은 이후 인덱스부터 시작합니다.

[^e1]: charShape 속성 목록: height(크기), textColor(색상), bold(굵기), italic(기울임), underlineType(밑줄), strikeoutShape(취소선), emphasis(강조점), ratio(장평), spacing(자간), relSz(상대크기), offset(세로위치), useKerning(커닝), useFontSpace(폰트 자간).
