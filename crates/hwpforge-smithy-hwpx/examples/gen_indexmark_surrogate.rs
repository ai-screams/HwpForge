//! Task #75 IndexMark surrogate-pair fixture — 이모지 primary 변형.
//!
//! HWP5 의 `idxm` wire 는 `primary[0]` (UTF-16 code unit 1개) 를
//! `properties.high` 에 packing 한다 (split-leader BSTR). primary 가
//! surrogate pair (이모지) 로 **시작**하면 high surrogate 만 header 에
//! 실리고 low surrogate 는 body 로 넘어가는데, 한컴이 실제로 이렇게
//! 쪼개 저장하는지 / 우리 재조립이 맞는지 fixture 로 검증한다
//! (Wave 12k 보류 항목).
//!
//! | # | label | primary | secondary | 검증 포인트 |
//! |---|-------|---------|-----------|-------------|
//! | 1 | bmp-baseline | 컴퓨터 | 하드웨어 | Wave 12k 관찰과 동일한 대조군 |
//! | 2 | emoji-first | 😀테스트 | (없음) | surrogate pair 가 split-leader 경계에 걸침 |
//! | 3 | emoji-only | 😀 | (없음) | primary 전체가 pair 1개 (2 units) |
//! | 4 | emoji-mid | 테스😀트 | (없음) | pair 가 body 영역 내부 (대조군) |
//! | 5 | emoji-secondary | 키워드 | 😀값 | secondary 쪽 pair (plain BSTR 경로) |
//!
//! 사용법:
//! ```text
//! cargo run -p hwpforge-smithy-hwpx --example gen_indexmark_surrogate -- \
//!     temp/sample-indexmark-surrogate.hwpx
//! ```
//! 이후 한컴에서 열어 `examples/hwp5_review/sample-indexmark-surrogate.hwp`
//! 로 다른 이름 저장.

use std::path::PathBuf;

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
use hwpforge_smithy_hwpx::HwpxEncoder;

/// (label, primary, secondary)
const VARIANTS: &[(&str, &str, Option<&str>)] = &[
    ("bmp-baseline", "컴퓨터", Some("하드웨어")),
    ("emoji-first", "😀테스트", None),
    ("emoji-only", "😀", None),
    ("emoji-mid", "테스😀트", None),
    ("emoji-secondary", "키워드", Some("😀값")),
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args();
    let _program = args.next();
    let output: PathBuf = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("temp/sample-indexmark-surrogate.hwpx"));

    let cs = CharShapeIndex::new(0);
    let ps = ParaShapeIndex::new(0);

    let paragraphs: Vec<Paragraph> = VARIANTS
        .iter()
        .map(|(label, primary, secondary)| {
            let prefix = format!("[{label}]: 본문");
            Paragraph::with_runs(
                vec![
                    Run::text(prefix.as_str(), cs),
                    Run::control(
                        Control::IndexMark {
                            primary: (*primary).to_string(),
                            secondary: secondary.map(str::to_string),
                        },
                        cs,
                    ),
                    Run::text(" 끝", cs),
                ],
                ps,
            )
        })
        .collect();

    let section = Section::with_paragraphs(paragraphs, PageSettings::a4());

    let mut doc = Document::new();
    doc.add_section(section);
    let validated = doc.validate()?;

    let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
    let image_store = ImageStore::new();
    let bytes = HwpxEncoder::encode(&validated, &store, &image_store)?;

    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&output, &bytes)?;
    println!(
        "Wrote {} ({} bytes) — {} indexmark variants",
        output.display(),
        bytes.len(),
        VARIANTS.len(),
    );
    Ok(())
}
