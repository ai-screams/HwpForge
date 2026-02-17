//! Generates a demo HWPX file from markdown.
//!
//! Run: `cargo run -p hwpforge-smithy-md --example gen_hwpx`

use hwpforge_blueprint::builtins::builtin_default;
use hwpforge_smithy_hwpx::{HwpxEncoder, HwpxStyleStore};
use hwpforge_smithy_md::MdDecoder;

fn main() {
    let markdown = r#"---
title: HwpForge 테스트 문서
author: Claude Code
date: 2026-02-16
---

# HwpForge Phase 5 데모

## 개요

이 문서는 HwpForge의 Markdown → HWPX 파이프라인으로 생성되었습니다.
LLM 에이전트가 마크다운을 작성하면 한글 문서로 자동 변환됩니다.

## 기능 목록

- GFM 마크다운 파싱 (pulldown-cmark)
- YAML Frontmatter 메타데이터
- Lossy/Lossless 양방향 인코딩
- Blueprint 스타일 템플릿 적용
- HWPX ZIP+XML 생성

### 표 예시

| 항목 | 설명 | 상태 |
|------|------|------|
| Phase 0 | Foundation | 완료 |
| Phase 1 | Core | 완료 |
| Phase 2 | Blueprint | 완료 |
| Phase 3 | HWPX Decoder | 완료 |
| Phase 4 | HWPX Encoder | 완료 |
| Phase 5 | Markdown Bridge | 완료 |

## 결론

HwpForge는 Rust 기반의 한글 문서 자동화 라이브러리입니다.
"#;

    let template = builtin_default().expect("builtin_default failed");
    let md_doc = MdDecoder::decode(markdown, &template).expect("MD decode failed");
    let validated = md_doc.document.validate().expect("validation failed");
    let store = HwpxStyleStore::from_registry(&md_doc.style_registry);
    let hwpx_bytes = HwpxEncoder::encode(&validated, &store).expect("HWPX encode failed");

    let out_path = "hwpforge_demo.hwpx";
    std::fs::write(out_path, &hwpx_bytes).expect("file write failed");
    println!("Generated: {} ({} bytes)", out_path, hwpx_bytes.len());
}
