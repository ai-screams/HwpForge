**HWPX 포맷 분석 보고서**

**HwpForge 프로젝트 기술 문서**

작성: HwpForge 개발팀

작성일: 2026년 2월 27일 — 버전 1.0

문서번호: HWPFORGE-2026-TECH-001

![image1](images/image1.png)

HwpForge는 한국의 HWP/HWPX 문서 포맷을 프로그래밍으로 제어하는 **순수 Rust 라이브러리**입니다. 프로젝트 마스코트인 <strong>오리너구리(Platypus)</strong>는 HWPX 포맷의 독특한 특성을 상징합니다 — 포유류이면서 알을 낳고, 부리와 독침을 가진 것처럼, HWPX도 XML 기반이면서 독자적인 네임스페이스와 규격을 가진 독특한 포맷입니다.

**1. HWPX 포맷 개요**

HWPX는 한컴오피스(한글)가 사용하는 **개방형 XML 문서 포맷**으로, **KS X 6101**

[^1]

표준을 기반으로 합니다. 내부 구조는 ZIP 압축 아카이브 안에 XML 파일들을 계층적으로 배치한 형태입니다.

HWPX는 Microsoft Office의 OOXML과 유사한 구조를 가지며, ZIP → XML → 네임스페이스 계층으로 이루어져 있습니다. HwpForge는 이 포맷의 완전한 인코드/디코드를 순수 Rust로 구현하였습니다.

| **경로** | **역할** |
| --- | --- |
| mimetype | MIME 타입 선언 |
| version.xml | HWPX 버전 정보 |
| Contents/header.xml | 스타일·글꼴·문단모양 정의 |
| Contents/section0.xml | 본문 섹션 XML |
| Contents/content.hpf | OPF 매니페스트 |
| META-INF/container.xml | ODF 컨테이너 진입점 |
| BinData/image\*.png | 이미지 바이너리 |
| Chart/chart\*.xml | 차트 데이터 (manifest 등록 금지!) |

특히 <strong>Chart/*.xml</strong> 파일은 content.hpf에 등록하면 **한글이 크래시**합니다. ZIP에만 존재해야 합니다.

<!-- hwpforge:section -->

**2. XML 구조 및 네임스페이스 체계**

HWPX의 XML은 6개의 네임스페이스 접두어로 구분됩니다. HwpForge는 serde의 rename 기능을 활용하여 각 네임스페이스를 Rust 구조체에 자동으로 매핑합니다.

**2.1 XML 네임스페이스 목록**

| **접두어** | **URI** | **주요 용도** |
| --- | --- | --- |
| hh: | urn:...hwpmlHead | header.xml 스타일/글꼴 |
| hp: | urn:...hwpmlPara | 문단, 표, 도형 |
| hc: | urn:...hwpmlCore | 도형 기하 (startPt, pt) |
| hs: | urn:...hwpmlSect | 섹션 설정 (페이지, 다단) |
| ha: | urn:...hwpmlApp | 앱 설정 |
| hm: | urn:...hwpmlMaster | 마스터 페이지 |

네임스페이스 선언(xmlns)은 각 XML 파일의 **루트 요소에만** 작성합니다.

[^2]

**2.2 파일별 역할 상세**

| **파일** | **루트 요소** | **HwpForge 모듈** |
| --- | --- | --- |
| Contents/header.xml | hh:head | encoder/header.rs |
| Contents/section0.xml | hs:sec | encoder/section.rs |
| Contents/content.hpf | opf:package | encoder/package.rs |
| META-INF/container.xml | container | encoder/package.rs |
| Chart/chart\*.xml | c:chartSpace | encoder/chart.rs |

<strong>Dual Serde Rename 패턴:</strong>

\#\[serde(rename(serialize = "hh:refList",                 deserialize = "refList"))\] <em>pub ref_list: HxRefList,</em>

<strong>2.3 Phase별 구현 규모 (코드 라인 수)</strong>

아래 차트는 HwpForge의 Phase 0-5에 걸친 소스 코드 증가 추이를 보여줍니다.

<!-- chart -->

<em>[차트 1] Phase별 소스 코드 라인 수 (Phase 0-5)</em>

<!-- chart -->

<em>[차트 2] 기능별 인코드/디코드 지원 현황</em>

<!-- hwpforge:section -->

<strong>3. 구현 주의사항 (Gotchas)</strong>

HwpForge 구현 과정에서 발견한 핵심 주의사항을 정리합니다.

**3.1 XML 인코딩 주의사항**

<strong>(1) BGR 색상 순서</strong>

HWP/HWPX 포맷은 <strong>BGR (Blue-Green-Red)</strong> 바이트 순서를 사용합니다. 빨강(Red)을 표현하려면 **0x0000FF**로 저장해야 합니다 (RGB에서는 0xFF0000). RGB로 착각하면 파란색이 됩니다.

HwpForge는 Color::from\_rgb(r, g, b) 생성자를 통해 내부적으로 BGR 변환을 처리합니다. 절대로 raw 16진수 값을 직접 사용하지 마십시오.

<strong>(2) ctrl 요소 순서 규칙</strong>

hp:sec (섹션) 내부의 ctrl 요소는 반드시 secPr → colPr → header → footer → pageNum 순서로 배치해야 합니다. 순서가 틀리면 한글이 섹션 설정을 무시하거나 크래시합니다.

<strong>(3) 도형 기하에는 hc: 네임스페이스</strong>

선(Line) 도형의 **startPt/endPt**와 다각형(Polygon)의 **pt** 요소는 반드시 <strong>hc:</strong> 네임스페이스를 사용해야 합니다. hp:를 사용하면 한글에서 파싱 오류가 발생합니다.

<strong>(4) HwpUnit 단위계</strong>

HWPX의 모든 길이 단위는 HWPUNIT으로, 1pt = 100 HWPUNIT, 1mm ≈ 283 HWPUNIT 관계를 가집니다. HwpUnit::from\_pt() 또는 HwpUnit::from\_mm()로 변환합니다.

<strong>(5) Chart XML manifest 등록 금지</strong>

Chart/\*.xml 파일은 ZIP 아카이브에만 존재해야 하며, content.hpf 매니페스트에 등록하면 한글이 크래시합니다. 또한 차트 데이터의 &lt;c:f&gt; formula 참조가 없으면 빈 차트가 표시됩니다.

<strong>(6) TextBox는 hp:rect 구조</strong>

HWPX에서 글상자(TextBox)는 &lt;hp:rect&gt; + &lt;hp:drawText&gt; 구조입니다. Control 요소가 아니며, 도형의 일종으로 처리해야 합니다. 꼭짓점(pt0\~pt3)은 hc: 네임스페이스를 사용합니다.

**3.2 도형 및 수식 주의사항**

<strong>(7) 수식: HancomEQN 포맷</strong>

HWPX 수식은 MathML이 아닌 HancomEQN 스크립트를 사용합니다. 수식에는 shape common 블록이 없습니다 (offset, orgSz, curSz 등 없음). flowWithText=1, outMargin=56이 기본값입니다.

$x=\frac{-b+-root{2}of{b^{2}-4 ac}}{2 a}$

<strong>(6) 타원 도형 — 주의 영역</strong>

<strong>BGR!</strong>

타원은 center, axis1, axis2 세 점으로 정의됩니다. 정원(circle)은 width=height로 설정합니다. shadow alpha는 도형별로 다릅니다: rect=178, line=0, ellipse=0.

<strong>(9) 다각형 — 첫 점 반복 필수</strong>

Polygon 꼭짓점 마지막에 첫 점을 반복해야 닫힌 도형이 됩니다. 반복하지 않으면 한글에서 마지막 변이 표시되지 않습니다.

<strong>주의!</strong>

<strong>(10) shapeComment 필수</strong>

모든 도형에는 &lt;hp:shapeComment&gt; 요소가 필수입니다. 사각형은 '사각형입니다.', 선은 '선입니다.', 타원은 '타원입니다.' 등 형태별 고정 문자열을 사용합니다.

<strong>(11) serde 필드 순서</strong>

quick-xml의 serde 직렬화에서 Rust 구조체의 필드 선언 순서가 XML 요소 순서를 결정합니다. 한글은 요소 순서에 민감하므로 golden fixture와 동일한 순서를 유지해야 합니다.

<!-- hwpforge:section -->

**4. 구현 현황 및 결론**

**4.1 Phase별 구현 현황**

HwpForge v1.0 기준 총 37,052 LOC, 988개 테스트, 8개 크레이트입니다.

| **Phase** | **크레이트** | **상태** | **테스트** | **LOC** |
| --- | --- | --- | --- | --- |
| 0 | foundation | 완료 (90+) | 224 | 4,432 |
| 1 | core | 완료 (94) | 331 | 5,554 |
| 2 | blueprint | 완료 (90) | 200 | 4,647 |
| 3 | decoder | 완료 (96) | 110 | 3,666 |
| 4 | encoder | 완료 (95) | 226 | 10,349 |
| 5 | smithy-md | 완료 (91) | 73 | 3,757 |
| Wave1-6 | 확장 기능 | 완료 | — | \~4,648 |

**4.2 테스트 수 성장 추이**

TDD 방식으로 개발되어 Phase 진행에 따라 테스트가 꾸준히 증가했습니다.

<!-- chart -->

<em>[차트 3] Phase별 누적 테스트 수 추이</em>

<!-- chart -->

<em>[차트 4] 크레이트별 코드량(LOC) 비율</em>

**4.3 향후 과제**

**Phase 6**: Python 바인딩 (PyO3) 및 CLI

**Phase 7**: MCP 서버 통합 — Claude Code 직접 연동

**Phase 8**: 종합 테스트 및 v1.0 릴리즈

**Phase 9**: HWPX 고급 기능 (OLE, 양식, 변경추적)

**Phase 10**: smithy-hwp5 — HWP5 바이너리 읽기

**4.4 결론**

HwpForge는 순수 Rust로 HWPX 포맷의 완전한 인코드/디코드를 구현한 최초의 오픈소스 프로젝트입니다.

Wave 1-6에서는 이미지, 머리글/바닥글, 각주/미주, 글상자, 다단, 도형, 캡션, 수식, 차트까지 확장하여 실무 문서 생성에 필요한 대부분의 기능을 갖추었습니다.

[^e1]

*— HwpForge 개발팀, 2026년 2월 —*

[^1]: KS X 6101은 한국산업표준(KS)으로 제정된 한글 문서 파일 포맷 규격입니다. 한국표준정보망(KSSN)을 통해 열람 가능하며, openhwp 프로젝트에 9,054줄 분량의 마크다운 사양이 공개되어 있습니다.
[^2]: HwpForge는 루트 요소를 수동으로 생성하고 내부 콘텐츠만 serde로 직렬화하는 'xmlns 래핑 패턴'을 사용합니다.
[^e1]: 본 보고서는 HwpForge의 모든 구현 API를 활용하여 작성되었습니다. 텍스트, 표, 이미지, 차트, 수식, 도형, 다단, 머리글/바닥글, 각주/미주, 글상자 등 17개 API 유형을 포함합니다.