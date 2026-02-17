//! Generates a demo HWPX file from markdown.
//!
//! Run: `cargo run -p hwpforge-smithy-md --example gen_hwpx`

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_smithy_md::MdDecoder;

fn main() {
    // Read from file if exists, otherwise use default
    let markdown = std::fs::read_to_string("/tmp/simple_test.md").unwrap_or_else(|_| {
        r#"# header

안녕하세요"#
            .to_string()
    });

    println!("📄 Input Markdown:");
    println!("{}", markdown);
    println!("\n{}\n", "=".repeat(60));

    let markdown_for_encode = if markdown.starts_with("---") {
        markdown
    } else {
        // Add minimal frontmatter if missing
        format!(
            r#"---
title: "Simple Test"
---

{}"#,
            markdown
        )
    };

    let _old_markdown = r#"---
title: "HwpForge Phase 4.1 개선사항 테스트"
author: "HwpForge Team"
date: "2026-02-17"
---

# Phase 4.1 개선사항 검증 문서

이 문서는 **Phase 4.1**에서 개선된 6가지 HWPX encoder 기능을 테스트합니다.

## 주요 개선 내용

### 1. 텍스트 렌더링 파라미터 (charPr 완전화)
- ✅ ratio (장평): 100%
- ✅ spacing (자간): 0
- ✅ relSz (상대 크기): 100%
- ✅ offset (오프셋): 0

7개 언어(한글, 영문, 한자, 일본어, 기타, 기호, 사용자)별 값이 모두 정의됩니다.

### 2. 문단 포맷팅 제어 (paraPr 완전화)
- ✅ heading: 개요 번호 매기기 연결
- ✅ breakSetting: 줄바꿈 규칙 (영문 단어 유지, 외톨이줄 방지)
- ✅ autoSpacing: 한글-영문/숫자 자동 간격
- ✅ border: 문단 테두리/배경

### 3. 글자 배경 정의 (borderFill id=2)
- ✅ fillBrush 추가
- ✅ winBrush 정의 (faceColor, hatchColor, alpha)

### 4. 탭 정의 확장 (tabProperties)
- ✅ id=0: 기본 탭
- ✅ id=1: 개요용 자동 탭 (autoTabLeft=1)

### 5. 한국어 개요 번호 시스템 (numberings)
- ✅ 7단계 자동 번호 매기기
- Level 1: 1. (아라비아 숫자)
- Level 2: 가. (한글 음절)
- Level 3: 1) (아라비아 숫자)
- Level 4: 가) (한글 음절)
- Level 5: (1) (아라비아 숫자)
- Level 6: (가) (한글 음절)
- Level 7: ① (원문자)

### 6. 단 레이아웃 (colPr)
- ✅ 1단 NEWSPAPER 레이아웃 명시

## 텍스트 포맷팅 테스트

**굵은 글씨**, *기울임*, ~~취소선~~ 모두 지원합니다.

## 목록 테스트

순서 없는 목록:
- 첫 번째 항목
- 두 번째 항목
- 세 번째 항목

순서 있는 목록:
1. 단계 1
2. 단계 2
3. 단계 3

## 표 테스트

| Gap | 요소 | Before | After | 상태 |
|-----|------|--------|-------|------|
| 1 | charPr | fontRef만 | +ratio/spacing/relSz/offset | ✅ |
| 2 | paraPr | align+margin | +heading/breakSetting/autoSpacing/border | ✅ |
| 3 | borderFill | 테두리만 | +fillBrush | ✅ |
| 4 | tabProperties | 1개 | 2개 | ✅ |
| 5 | numberings | 없음 | 7단계 | ✅ |
| 6 | colPr | 없음 | NEWSPAPER | ✅ |

## 결론

**모든 6가지 개선사항이 이 HWPX 파일에 포함되어 있습니다!**

- 테스트: 265/265 통과
- Clippy: Zero warnings
- Golden roundtrip: 8/8 통과

한글 프로그램에서 이 파일을 열어 정상 작동을 확인하세요. ✅
"#;

    let template = builtin_default().expect("builtin_default failed");
    let md_doc = MdDecoder::decode(&markdown_for_encode, &template).expect("MD decode failed");
    let validated = md_doc.document.validate().expect("validation failed");
    let store = HwpxStyleStore::from_registry(&md_doc.style_registry);
    let images = hwpforge_core::image::ImageStore::new();
    let hwpx_bytes = HwpxEncoder::encode(&validated, &store, &images).expect("HWPX encode failed");

    let out_path = "/Users/hanyul/Works/AiScream/HwpForge/Phase4_1_Improvements_Test.hwpx";
    std::fs::write(out_path, &hwpx_bytes).expect("file write failed");
    println!("✅ Generated: {}", out_path);
    println!("   Size: {} bytes", hwpx_bytes.len());
    println!("\n📂 파일 위치: {}", out_path);
    println!("🎯 한글 프로그램에서 열어보세요!");
}
