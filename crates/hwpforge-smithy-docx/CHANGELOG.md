# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- 번호 글꼴의 첫 줄 높이 증가를 기존 Word 글꼴 metrics와 줄 간격 규칙으로 계산해 문단 앞 간격에 더한다. 고정 줄 간격과 metrics 미지원 글꼴은 기존 간격을 유지한다. 양수 첫 줄 들여쓰기·폭 0 칸처럼 배치가 미지원인 목록도 높이 보정과 `list_marker_script`·`list_marker_line_height` 진단을 적용하되 출력에 쓰이지 않는 번호 글자 모양은 등록하지 않는다.

- 단일·이중 바이트 폭 균형 호환 설정이 켜진 DOCX는 모든 글자 모양에서 반각 공백을 사용하고, 꺼지거나 생략되면 글꼴 공백 폭을 유지한다.

- 유효 문단 자동 간격을 독립적으로 상속하고 출력에 전달하며, 정확히 보존되는 꺼짐 설정의 근사 경고를 제거한다.

- 걸침 0·번호 뒤 탭인 목록은 설정의 기본 탭 간격(생략 시 720twip)을 적용한다. 자기 수준 decimal 토큰과 숫자·점 리터럴, 원본 Symbol 글머리만 측정 비율로 추정한다. 오른쪽 정렬은 anchor도 탭 후보로 사용하므로 거리가 0이다.
- 폭은 글꼴·굵기·기울임에 같은 비율을 재사용하며 셀·다단의 로컬 원점과 걸침 0 가운데 정렬은 미측정 근사다. 경계 위험을 진단한다. 명시적 목록 배치는 해석 완료된 원래 left와 first를 보존하며 후속 줄 손실을 선제 경고하지 않는다. 미지원 문자·활성 수준 번호 탭·걸침 양수는 기존 정책을 유지하고 일반 본문 탭 위치는 적용하지 않는다.

- 문단에 직접 번호가 지정되면 번호 수준과 재정의의 들여쓰기를 문단 스타일보다 우선한다. 직접 문단 들여쓰기는 최우선이며, 스타일에서 상속된 번호와 들여쓰기 밖의 속성 순서는 유지한다.
