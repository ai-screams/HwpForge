# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- DocxDecoded.registry includes original positive solid border widths with explicit source sizes in the supported range for source-admitted tables. Tables with positive exact rows, unsupported source formatting or boundaries that cannot be represented by four sides omit widths and report the existing DOCX table-border warning. Source uncertainty follows the selected style inheritance chain, including unhandled row formatting, and table/row/cell properties. Omitted internal cell borders retain unsupported source attributes in the admission decision. Unsupported direction, placement values, unknown measurement/layout values and unresolved style links defer admission even when later properties override them. Pagination-only options and ignored header-cell associations do not affect admission. Known preferred-width approximations, vertical content alignment and background paint retain admission. Core cell margins remain source values; row minima use the Word content-height calculation and the existing border budget. Width-bearing and widthless definitions with the same appearance are distinct.

- 번호 글꼴의 첫 줄 높이 증가를 기존 Word 글꼴 metrics와 줄 간격 규칙으로 계산해 문단 앞 간격에 더한다. 표 셀의 최소 높이에는 목록 문단마다 이 증가분을 한 번씩 포함한다. 고정 줄 간격과 metrics 미지원 글꼴은 기존 간격을 유지한다. 양수 첫 줄 들여쓰기·폭 0 칸처럼 배치가 미지원인 목록도 높이 보정과 `list_marker_script`·`list_marker_line_height` 진단을 적용하되 출력에 쓰이지 않는 번호 글자 모양은 등록하지 않는다.

- 단일·이중 바이트 폭 균형 호환 설정이 켜진 DOCX는 모든 글자 모양에서 반각 공백을 사용하고, 꺼지거나 생략되면 글꼴 공백 폭을 유지한다.

- 유효 문단 자동 간격을 독립적으로 상속하고 출력에 전달하며, 정확히 보존되는 꺼짐 설정의 근사 경고를 제거한다.

- 걸침 0·번호 뒤 탭인 목록은 설정의 기본 탭 간격(생략 시 720twip)을 적용한다. 자기 수준 decimal 토큰과 숫자·점 리터럴, 원본 Symbol 글머리만 측정 비율로 추정한다. 오른쪽 정렬은 anchor도 탭 후보로 사용하므로 거리가 0이다.
- 폭은 글꼴·굵기·기울임에 같은 비율을 재사용하며 셀·다단의 로컬 원점과 걸침 0 가운데 정렬은 미측정 근사다. 경계 위험을 진단한다. 명시적 목록 배치는 해석 완료된 원래 left와 first를 보존하며 후속 줄 손실을 선제 경고하지 않는다. 걸침 0 경로의 미지원 문자와 활성 수준 번호 탭은 기존 정책을 유지하며 일반 본문 탭 위치는 적용하지 않는다.

- 문단에 직접 번호가 지정되면 번호 수준과 재정의의 들여쓰기를 문단 스타일보다 우선한다. 직접 문단 들여쓰기는 최우선이며, 스타일에서 상속된 번호와 들여쓰기 밖의 속성 순서는 유지한다.

### Fixed

- Cell content minima and atLeast spacing exclude the paragraph-end mark from a final line containing text. Empty paragraphs and trailing empty lines retain their existing mark-height calculation. Core retains source paragraph-end formatting, including formatting used by list markers. The paragraph-mark diagnostic describes source-format preservation rather than promising unchanged line height.

- 미지 폭 종류와 pct의 기존 근사를 유지하고, 완전한 grid를 가진 중첩 dxa·auto·nil 및 폭 미지정 표의 원본 열 너비를 유지한다. 실제 grid를 사용하는 경우 컨테이너 근사 진단을 내지 않는다.

- 확인된 Word 호환 모드의 표 맞춤·들여쓰기·첫 칸 왼쪽 여백 배치를 보존하며, 모드 미지원에는 기존 배치와 진단을 유지한다. 모드 누락은 최상위 표가 있는 문서에서 한 번 진단한다.

- 표 앞뒤와 표 칸 안에 이어지는 Word 번호 목록의 값을 보존한다. 표를 품은 문단과 칸의 출력 순서에 맞춰 필요한 번호 정의를 나누고 `list_numbering_split`으로 알린다. 나뉜 구간의 시작값은 고정되므로 앞 구간의 항목 삽입·삭제가 뒤 구간에 자동 전파되지 않는다.

- Word 목록의 공유 카운터, 수준별 첫 시작값 덮어쓰기와 재시작을 보존한다. 십진 자기 번호의 생략되거나 0인 시작값은 고정 리터럴 표식으로 가져오며, 필요한 경우 시작값이 고정된 번호 정의로 나누고 `DOCX_VALUE_APPROXIMATED`의 `list_numbering_split`으로 알린다.
- 상위 수준을 참조하는 복합 번호와 법률식 번호를 보존한다. 법률식 번호는 참조 형식을 십진으로 바꾼 정의 사본을 사용한다. 정의 상한으로 추가 분리가 불가능하면 활성 정의를 유지하고 남는 값·형식 차이를 `list_numbering_budget`으로 알린다. 필요한 사본을 얻지 못한 법률식 문단이 원래 참조 형식으로 나갈 때만 `list_number_legal`도 알린다. 복합·비십진 번호의 0값 근사는 `list_number_zero`로 알린다.
- `list_numbering_budget`은 추가 분리가 중지된 그룹당 한 번 기록한다. 경고 횟수로 영향 문단 수를 알 수는 없다.

- 양수 걸침 칸이 있는 LEFT·Tab 목록에서 유효 Latin 글꼴이 맑은 고딕 보통체이고 십진 숫자·마침표 표식을 측정할 수 있으면, 칸을 확실히 넘는 번호의 본문을 다음 기본 탭으로 이동합니다. 원래 후속 줄 위치는 유지합니다. 측정 오차 띠 안이거나 다음 탭 경계가 위험하면 기존 칸과 `list_marker_overflow` 경고를 유지합니다. 측정하지 못하는 글꼴·서식은 기존 명목 폭 판정을 따릅니다.

### Changed

- 가져온 문서의 조판 프로필을 `WordCompatible`로 정한다. 백분율 줄 간격은 글꼴 줄 높이의 배수 그대로 전달하고, 세로 정렬을 정하지 않은 셀은 Word 기본값인 위쪽 정렬로 쓴다. 표는 원본 순서에서 뒤따르는 문단에 붙인다.
- 모든 행의 높이가 exact인 표는 표 크기를 고정해 행 높이를 재현한다. 이때 한컴에서 표 크기 조절과 행 자동 확장이 잠기고 넘치는 글은 Word처럼 잘린다. exact 행과 다른 행이 섞인 표는 전처럼 exact 높이를 최소 높이로 근사한다. 두 경우 모두 `row_height` 진단으로 알린다.
